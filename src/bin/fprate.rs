//! False-positive rate of the filter at a fixed catalog size, measured over billions of distinct
//! absent keys on all cores. Counts do not depend on timing, so no host locking is needed.
//!   cargo run --release --bin fprate -- --bits 16 --hash blake3-xxh --probes 1000000000 --output results/fp/x.json
use cuckoo_bench::dataset::{encode, generate_barcodes, parse};
use cuckoo_bench::digest::{Blake3Xxh, DigestFn, Xxh3Direct};
use cuckoo_bench::filter::*;
use cuckoo_bench::stats::wilson_interval;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;

/// Count of 12-digit bases in [10^11, 10^12).
const SPACE: u64 = 900_000_000_000;
/// Prime, hence coprime to SPACE: `i -> STRIDE * i + offset (mod SPACE)` is injective for i < SPACE,
/// so no probe repeats.
const STRIDE: u64 = 1_000_000_007;

struct Args {
    count: usize,
    capacity: usize,
    bits: u32,
    hash: String,
    seeds: Vec<u64>,
    probes: u64,
    threads: usize,
    output: String,
}

fn parse_args() -> Args {
    let mut a = Args {
        count: 871_000,
        capacity: 900_000,
        bits: 16,
        hash: "blake3-xxh".into(),
        seeds: vec![0, 1, 2],
        probes: 1_000_000_000,
        threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        output: String::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--count" => a.count = val().parse().unwrap(),
            "--capacity" => a.capacity = val().parse().unwrap(),
            "--bits" => a.bits = val().parse().unwrap(),
            "--hash" => a.hash = val(),
            "--seeds" => a.seeds = val().split(',').map(|s| s.parse().unwrap()).collect(),
            "--probes" => a.probes = val().parse().unwrap(),
            "--threads" => a.threads = val().parse().unwrap(),
            "--output" => a.output = val(),
            other => panic!("unknown flag {other}"),
        }
    }
    assert!(!a.output.is_empty() && a.count <= a.capacity && a.probes < SPACE);
    a
}

fn run_seed<T: Table + Sync, H: DigestFn>(a: &Args, seed: u64) -> Value {
    let keys = generate_barcodes(a.count, seed);
    let key_bases: HashSet<u64> = keys.iter().map(|k| parse(k) / 10).collect();
    let mut cf = CuckooFilter::<T>::new(a.capacity, 4, 500, seed);
    for k in &keys {
        assert!(cf.insert(H::digest(k).as_slice()), "insertion failed");
    }
    assert!(keys.iter().all(|k| cf.contains(H::digest(k).as_slice())), "false negative");
    let offset = (seed as u128 * 7_919_000_000_017 % SPACE as u128) as u64;
    let chunk = a.probes / a.threads as u64;
    let (tested, skipped, hits) = std::thread::scope(|s| {
        let (cf, key_bases) = (&cf, &key_bases);
        let handles: Vec<_> = (0..a.threads as u64)
            .map(|t| {
                let (lo, hi) = (t * chunk, if t + 1 == a.threads as u64 { a.probes } else { (t + 1) * chunk });
                s.spawn(move || {
                    let (mut tested, mut skipped, mut hits) = (0u64, 0u64, 0u64);
                    for i in lo..hi {
                        let base = 100_000_000_000 + ((STRIDE as u128 * i as u128 + offset as u128) % SPACE as u128) as u64;
                        if key_bases.contains(&base) {
                            skipped += 1;
                            continue;
                        }
                        tested += 1;
                        hits += cf.contains(H::digest(&encode(base)).as_slice()) as u64;
                    }
                    (tested, skipped, hits)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).fold((0, 0, 0), |x, y| (x.0 + y.0, x.1 + y.1, x.2 + y.2))
    });
    let load = cf.count as f64 / cf.total_slots() as f64;
    let ideal = 2.0 * cf.b as f64 * load / ((1u128 << T::BITS) - 1) as f64;
    let rate = hits as f64 / tested as f64;
    eprintln!("seed {seed}: {hits} / {tested} = {rate:.3e}  (ideal {ideal:.3e}, ratio {:.2})", rate / ideal);
    json!({"seed": seed, "items": cf.count, "buckets": cf.num_buckets, "load_factor": load,
           "probes_tested": tested, "skipped_in_key_set": skipped, "false_positives": hits,
           "rate": rate, "wilson_95_interval": wilson_interval(hits, tested),
           "ideal_rate": ideal, "observed_over_ideal": rate / ideal})
}

fn run<T: Table + Sync, H: DigestFn>(a: &Args) -> Value {
    let runs: Vec<Value> = a.seeds.iter().map(|&s| run_seed::<T, H>(a, s)).collect();
    let sum = |k: &str| runs.iter().map(|r| r[k].as_u64().unwrap()).sum::<u64>();
    let (tested, hits) = (sum("probes_tested"), sum("false_positives"));
    let ideal = runs[0]["ideal_rate"].as_f64().unwrap();
    let rate = hits as f64 / tested as f64;
    eprintln!("pooled bits={} {}: {hits} / {tested} = {rate:.3e}  (ideal {ideal:.3e}, ratio {:.2})", T::BITS, H::NAME, rate / ideal);
    json!({"configuration": {"count": a.count, "capacity": a.capacity, "fingerprint_bits": T::BITS, "hash_path": H::NAME,
                             "probes_per_seed": a.probes, "threads": a.threads, "slots_per_bucket": 4, "max_kicks": 500},
           "runs": runs,
           "pooled": {"probes_tested": tested, "false_positives": hits, "rate": rate,
                      "wilson_95_interval": wilson_interval(hits, tested), "ideal_rate": ideal,
                      "observed_over_ideal": rate / ideal}})
}

fn main() {
    let a = parse_args();
    let result = match (a.bits, a.hash.as_str()) {
        (16, "blake3-xxh") => run::<Table16, Blake3Xxh>(&a),
        (24, "blake3-xxh") => run::<Table24, Blake3Xxh>(&a),
        (32, "blake3-xxh") => run::<Table32, Blake3Xxh>(&a),
        (64, "blake3-xxh") => run::<Table64, Blake3Xxh>(&a),
        (16, "xxh3") => run::<Table16, Xxh3Direct>(&a),
        (24, "xxh3") => run::<Table24, Xxh3Direct>(&a),
        (32, "xxh3") => run::<Table32, Xxh3Direct>(&a),
        (64, "xxh3") => run::<Table64, Xxh3Direct>(&a),
        _ => panic!("--bits must be 16/24/32/64 and --hash blake3-xxh or xxh3"),
    };
    let mut f = OpenOptions::new().write(true).create_new(true).open(&a.output).expect("output file already exists");
    f.write_all((serde_json::to_string_pretty(&result).unwrap() + "\n").as_bytes()).unwrap();
}
