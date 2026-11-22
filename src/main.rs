//! Warm-cache membership benchmark: cuckoo filter vs SQLite vs sorted packed array.
//! Run pinned to one core (taskset) with a fixed-frequency governor; use bench.sh.
use cuckoo_bench::baselines::{export_sqlite, open_readonly, SortedArray};
use cuckoo_bench::dataset::{generate_barcodes, Barcode};
use cuckoo_bench::digest::{Blake3Xxh, Digest, DigestFn, Xxh3Direct};
use cuckoo_bench::filter::*;
use cuckoo_bench::stats::{mad, median, wilson_interval};
use rand::{Rng, RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

struct Args {
    count: usize,
    queries: usize,
    repeats: usize,
    warmups: usize,
    seed: u64,
    hit_rate: f64,
    bits: u32,
    hash: String,
    output: PathBuf,
    no_sqlite: bool,
    hist_passes: usize,
}

fn parse_args() -> Args {
    let mut a = Args {
        count: 900_000,
        queries: 100_000,
        repeats: 15,
        warmups: 2,
        seed: 0,
        hit_rate: 0.5,
        bits: 16,
        hash: "blake3-xxh".into(),
        output: PathBuf::new(),
        no_sqlite: false,
        hist_passes: 5,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = |name: &str| it.next().unwrap_or_else(|| panic!("{name} needs a value"));
        match flag.as_str() {
            "--count" => a.count = val(&flag).parse().unwrap(),
            "--queries" => a.queries = val(&flag).parse().unwrap(),
            "--repeats" => a.repeats = val(&flag).parse().unwrap(),
            "--warmups" => a.warmups = val(&flag).parse().unwrap(),
            "--seed" => a.seed = val(&flag).parse().unwrap(),
            "--hit-rate" => a.hit_rate = val(&flag).parse().unwrap(),
            "--fingerprint-bits" => a.bits = val(&flag).parse().unwrap(),
            "--hash" => a.hash = val(&flag),
            "--output" => a.output = val(&flag).into(),
            "--hist-passes" => a.hist_passes = val(&flag).parse().unwrap(),
            "--no-sqlite" => a.no_sqlite = true,
            other => panic!("unknown flag {other}"),
        }
    }
    assert!(!a.output.as_os_str().is_empty(), "--output is required");
    assert!(a.count > 0 && a.queries > 0 && a.repeats > 0 && (0.0..=1.0).contains(&a.hit_rate));
    a
}

fn shuffle<X, R: Rng>(v: &mut [X], rng: &mut R) {
    for i in (1..v.len()).rev() {
        v.swap(i, rng.random_range(0..=i));
    }
}

/// Times one pass of `n` calls; the result is consumed so nothing is optimised away.
#[inline(never)]
fn time_batch<F: FnMut(usize) -> bool>(n: usize, mut f: F) -> (u64, u64) {
    let start = Instant::now();
    let mut found = 0u64;
    for i in 0..n {
        found += black_box(f(black_box(i))) as u64;
    }
    (start.elapsed().as_nanos() as u64, found)
}

// ---- per-call latency via serialised TSC reads (x86_64) ----
#[inline(always)]
fn tsc_now() -> u64 {
    unsafe {
        core::arch::x86_64::_mm_lfence();
        let t = core::arch::x86_64::_rdtsc();
        core::arch::x86_64::_mm_lfence();
        t
    }
}

fn tsc_ghz() -> f64 {
    let (t0, w0) = (tsc_now(), Instant::now());
    while w0.elapsed().as_millis() < 200 {}
    (tsc_now() - t0) as f64 / w0.elapsed().as_nanos() as f64
}

#[inline(never)]
fn latency_cycles<F: FnMut(usize) -> bool>(n: usize, passes: usize, mut f: F) -> Vec<u64> {
    let mut out = Vec::with_capacity(n * passes);
    for _ in 0..passes {
        for i in 0..n {
            let t0 = tsc_now();
            black_box(f(black_box(i)));
            out.push(tsc_now() - t0);
        }
    }
    out
}

fn percentiles(mut cycles: Vec<u64>, ghz: f64, overhead: f64) -> Value {
    cycles.sort_unstable();
    let at = |p: f64| cycles[((cycles.len() - 1) as f64 * p) as usize] as f64;
    let ns = |c: f64| ((c - overhead).max(0.0)) / ghz;
    json!({"samples": cycles.len(), "timer_overhead_ns": overhead / ghz,
           "p50_ns": ns(at(0.5)), "p90_ns": ns(at(0.9)), "p99_ns": ns(at(0.99)),
           "p999_ns": ns(at(0.999)), "max_ns": ns(at(1.0))})
}

#[derive(Clone, Copy)]
enum Method {
    FilterLookup,
    FilterHash,
    Array,
    Sqlite(usize),
    FilterSqlite(usize),
}

fn sha256_hex(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn locked_version(lock: &str, name: &str) -> String {
    let needle = format!("name = \"{name}\"\nversion = \"");
    lock.find(&needle)
        .map(|i| lock[i + needle.len()..].split('"').next().unwrap().to_string())
        .unwrap_or_default()
}

fn read_trim(path: &str) -> String {
    std::fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn environment() -> Value {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let model = cpuinfo.lines().find(|l| l.starts_with("model name")).map(|l| l.split(':').nth(1).unwrap().trim().to_string());
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let allowed = status.lines().find(|l| l.starts_with("Cpus_allowed_list")).map(|l| l.split(':').nth(1).unwrap().trim().to_string()).unwrap_or_default();
    let first_cpu = allowed.split(['-', ',']).next().unwrap_or("0").to_string();
    let lock = include_str!("../Cargo.lock");
    json!({
        "rustc": env!("RUSTC_VERSION"),
        "debug_assertions": cfg!(debug_assertions),
        "cpu_model": model,
        "cpus_allowed": allowed,
        "governor": read_trim(&format!("/sys/devices/system/cpu/cpu{first_cpu}/cpufreq/scaling_governor")),
        "scaling_cur_khz": read_trim(&format!("/sys/devices/system/cpu/cpu{first_cpu}/cpufreq/scaling_cur_freq")),
        "intel_no_turbo": read_trim("/sys/devices/system/cpu/intel_pstate/no_turbo"),
        "kernel": read_trim("/proc/sys/kernel/osrelease"),
        "sqlite": rusqlite::version(),
        "dependencies": {"blake3": locked_version(lock, "blake3"), "xxhash-rust": locked_version(lock, "xxhash-rust"),
                         "rusqlite": locked_version(lock, "rusqlite")},
    })
}

fn run<T: Table, H: DigestFn>(a: &Args) -> Value {
    std::fs::create_dir_all(a.output.parent().unwrap_or(".".as_ref())).unwrap();
    std::fs::create_dir(&a.output).expect("output directory already exists");
    let mut rng = ChaCha8Rng::seed_from_u64(a.seed);
    let corpus = generate_barcodes(a.count + a.queries, a.seed);
    let (keys, negatives) = corpus.split_at(a.count);
    let positives: Vec<Barcode> = (0..a.queries).map(|_| keys[rng.random_range(0..keys.len())]).collect();
    let positive_count = (a.queries as f64 * a.hit_rate).round() as usize;
    let mut mixed: Vec<Barcode> = positives[..positive_count].iter().chain(&negatives[..a.queries - positive_count]).copied().collect();
    shuffle(&mut mixed, &mut rng);
    let workloads: [(&str, Vec<Barcode>); 3] = [("positive", positives), ("negative", negatives.to_vec()), ("mixed", mixed)];

    // Filter build and correctness.
    let mut cf = CuckooFilter::<T>::new(a.count, 4, 500, a.seed);
    let t = Instant::now();
    for k in keys {
        assert!(cf.insert(H::digest(k).as_slice()), "insertion failed; no benchmark results accepted");
    }
    let build_ns = t.elapsed().as_nanos() as u64;
    assert!(keys.iter().all(|k| cf.contains(H::digest(k).as_slice())), "filter has false negatives");
    let neg_fp: Vec<bool> = negatives.iter().map(|k| cf.contains(H::digest(k).as_slice())).collect();
    let false_positives = neg_fp.iter().filter(|&&x| x).count() as u64;

    let array = SortedArray::build(keys);
    let mut build = json!({
        "filter": {"build_ns": build_ns, "table_bytes": cf.table.payload_bytes(), "items": cf.count,
                   "buckets": cf.num_buckets, "load_factor": cf.count as f64 / cf.total_slots() as f64,
                   "total_kicks": cf.total_kicks},
        "sorted_array": {"table_bytes": array.payload_bytes()},
    });

    let mut sqlite_names = vec![];
    let mut cons = vec![];
    if !a.no_sqlite {
        for (name, without_rowid) in [("sqlite_rowid", false), ("sqlite_without_rowid", true)] {
            let path = a.output.join(format!("{name}.db"));
            let t = Instant::now();
            let bytes = export_sqlite(keys, &path, without_rowid).unwrap();
            build[name] = json!({"build_ns": t.elapsed().as_nanos() as u64, "file_bytes": bytes});
            cons.push(open_readonly(&path).unwrap());
            sqlite_names.push(name);
        }
    }
    let mut stmts: Vec<rusqlite::Statement> = cons.iter().map(|c| c.prepare("SELECT 1 FROM barcodes WHERE barcode = ?1").unwrap()).collect();

    // Exact baselines are verified outside timed regions.
    assert!(keys.iter().all(|k| array.contains(k)) && !negatives.iter().any(|k| array.contains(k)), "incorrect sorted array");
    for s in stmts.iter_mut() {
        let sql = |s: &mut rusqlite::Statement, k: &Barcode| s.exists([std::str::from_utf8(k).unwrap()]).unwrap();
        assert!(keys.iter().all(|k| sql(s, k)) && !negatives.iter().any(|k| sql(s, k)), "incorrect sqlite baseline");
    }

    let mut methods: Vec<(String, Method)> = vec![("filter_lookup".into(), Method::FilterLookup), ("filter_hash_lookup".into(), Method::FilterHash), ("sorted_array".into(), Method::Array)];
    for (j, n) in sqlite_names.iter().enumerate() {
        methods.push((n.to_string(), Method::Sqlite(j)));
        methods.push((format!("filter_plus_{n}"), Method::FilterSqlite(j)));
    }

    let ghz = tsc_ghz();
    let overhead = {
        let mut v = latency_cycles(1000, 20, |_| true);
        v.sort_unstable();
        v[v.len() / 2] as f64
    };
    let mut records: Vec<Value> = vec![];
    let mut summary: Vec<Value> = vec![];
    let mut latency: Vec<Value> = vec![];

    for (wname, queries) in &workloads {
        let n = queries.len();
        let digests: Vec<Digest> = queries.iter().map(H::digest).collect();
        let expected_exact = match *wname {
            "positive" => n,
            "negative" => 0,
            _ => positive_count,
        };
        let mut run_one = |m: Method| -> (u64, u64) {
            match m {
                Method::FilterLookup => time_batch(n, |i| cf.contains(digests[i].as_slice())),
                Method::FilterHash => time_batch(n, |i| cf.contains(H::digest(&queries[i]).as_slice())),
                Method::Array => time_batch(n, |i| array.contains(&queries[i])),
                Method::Sqlite(j) => time_batch(n, |i| stmts[j].exists([std::str::from_utf8(&queries[i]).unwrap()]).unwrap()),
                Method::FilterSqlite(j) => time_batch(n, |i| {
                    cf.contains(H::digest(&queries[i]).as_slice()) && stmts[j].exists([std::str::from_utf8(&queries[i]).unwrap()]).unwrap()
                }),
            }
        };
        for (_, m) in &methods {
            for _ in 0..a.warmups {
                run_one(*m);
            }
        }
        let mut samples: Vec<Vec<f64>> = vec![vec![]; methods.len()];
        for repeat in 0..a.repeats {
            let mut order: Vec<usize> = (0..methods.len()).collect();
            shuffle(&mut order, &mut rng);
            for (position, &mi) in order.iter().enumerate() {
                let (name, m) = &methods[mi];
                let (elapsed, found) = run_one(*m);
                let exact = !matches!(m, Method::FilterLookup | Method::FilterHash);
                if exact {
                    assert_eq!(found as usize, expected_exact, "timed result mismatch: {name}");
                } else if *wname == "positive" {
                    assert_eq!(found as usize, n, "false negative in timed run");
                }
                let nsq = elapsed as f64 / n as f64;
                samples[mi].push(nsq);
                records.push(json!({"workload": wname, "method": name, "repeat": repeat, "order": position,
                                    "queries": n, "found": found, "elapsed_ns": elapsed, "ns_per_query": nsq}));
            }
        }
        for (mi, (name, _)) in methods.iter().enumerate() {
            let s = &samples[mi];
            summary.push(json!({"workload": wname, "method": name, "median_ns_per_query": median(s),
                                "mad_ns_per_query": mad(s), "min_ns_per_query": s.iter().cloned().fold(f64::MAX, f64::min),
                                "max_ns_per_query": s.iter().cloned().fold(0.0, f64::max)}));
        }
        if a.hist_passes > 0 {
            latency.push(json!({"workload": wname, "method": "filter_lookup",
                "latency": percentiles(latency_cycles(n, a.hist_passes, |i| cf.contains(digests[i].as_slice())), ghz, overhead)}));
            latency.push(json!({"workload": wname, "method": "sorted_array",
                "latency": percentiles(latency_cycles(n, a.hist_passes, |i| array.contains(&queries[i])), ghz, overhead)}));
        }
    }

    let sources = [include_str!("main.rs"), include_str!("filter.rs"), include_str!("baselines.rs"), include_str!("dataset.rs"), include_str!("stats.rs"), include_str!("../Cargo.toml"), include_str!("../Cargo.lock")];
    let result = json!({
        "schema_version": 2,
        "completed_at_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        "source_sha256": sha256_hex(&sources),
        "environment": environment(),
        "configuration": {"count": a.count, "queries_per_workload": a.queries, "repeats": a.repeats, "warmups": a.warmups,
                          "seed": a.seed, "requested_hit_rate": a.hit_rate, "actual_mixed_hit_rate": positive_count as f64 / a.queries as f64,
                          "slots_per_bucket": cf.b, "max_kicks": cf.max_kicks, "fingerprint_bits": T::BITS, "hash_path": H::NAME,
                          "tsc_ghz": ghz, "cache_regime": "warm",
                          "sqlite_pragmas": {"cache_size": -2048, "mmap_size": 0}},
        "dataset": {"sha256": sha256_hex(&[corpus.iter().map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("\n").as_str()]),
                    "generator": "ChaCha8 unique synthetic EAN-13; corpus split into inserted and disjoint negatives"},
        "correctness": {"false_negatives": 0, "negative_queries": a.queries, "false_positives": false_positives,
                        "false_positive_rate": false_positives as f64 / a.queries as f64,
                        "wilson_95_interval": wilson_interval(false_positives, a.queries as u64)},
        "build": build, "records": records, "summary": summary, "latency": latency,
    });
    std::fs::write(a.output.join("results.json"), serde_json::to_string_pretty(&result).unwrap() + "\n").unwrap();
    result
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
        _ => panic!("--fingerprint-bits must be 16/24/32/64 and --hash blake3-xxh or xxh3"),
    };
    println!("{} bits={} hash={} fp={}/{} load={:.4}", a.output.display(), a.bits, a.hash,
             result["correctness"]["false_positives"], a.queries, result["build"]["filter"]["load_factor"].as_f64().unwrap());
    for s in result["summary"].as_array().unwrap() {
        println!("{:9} {:34} median {:9.1} ns  mad {:7.1}", s["workload"].as_str().unwrap(), s["method"].as_str().unwrap(),
                 s["median_ns_per_query"].as_f64().unwrap(), s["mad_ns_per_query"].as_f64().unwrap());
    }
}
