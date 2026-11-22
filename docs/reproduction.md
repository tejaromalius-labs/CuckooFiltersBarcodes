# Reproducing the experiments

This guide gets you from a fresh checkout to the same measurements. The commands assume
a Linux shell. Only the timing runs need `sudo`, and only to lock the CPU's frequency.

## 1. Build and test

Install a recent Rust toolchain (the recorded runs used rustc 1.98.1), then, from the
project root:

```sh
cargo test --release
```

`tests/filter.rs` checks the filter against the 320 reference vectors in
`tests/golden.txt` (all four widths), rollback after a failed insert, deletes, duplicate
limits, reproducibility from a seed, and EAN-13 validity. The build uses
`-C target-cpu=native` (from `.cargo/config.toml`), fat LTO, one codegen unit and
`panic = "abort"`. That makes the binary fast on the machine that built it, and not
portable to other CPUs.

## 2. A small benchmark

```sh
cargo run --release -- --count 100000 --queries 20000 --repeats 3 --output results/smoke
```

The output folder must not exist yet; a run never overwrites earlier results. Options:
`--count`, `--queries`, `--repeats` (default 15), `--warmups` (2), `--seed`,
`--hit-rate` (0.5), `--fingerprint-bits` (16, 24, 32, or 64), `--hash` (`blake3-xxh` or
`xxh3`), `--no-sqlite`, `--hist-passes`, and `--output`. A run prints a median/MAD summary
and writes `results.json` plus two SQLite files. Smoke runs on an unlocked machine are
fine for checking things work, but their timings aren't comparable with the recorded ones.

## 3. Lock the machine for real timings

`bench.sh` automates the steps the recorded runs used. `setup` and `restore` call `sudo`:

```sh
./bench.sh setup                    # performance governor, turbo off, CPU 5 offline
./bench.sh run results/run1         # 16 runs, pinned to CPU 4
./bench.sh run results/run1 xxh3    # only the named hash path
./bench.sh large results/large1     # 20,000,000 keys, no SQLite
./bench.sh restore                  # put everything back
```

`setup` saves the previous state in `.host-state` and `restore` reads it back. The script
assumes CPU 4 is a P-core and CPU 5 is its hyper-threading sibling; check `lscpu -e` and
edit `CPU` and `SIBLING` at the top of `bench.sh` for another processor. With turbo off
the clock holds at the base frequency, so absolute times are conservative. The standard
set is widths {16, 24, 32, 64} × seeds {0, 1} × hashes {`blake3-xxh`, `xxh3`} at 900,000
keys, 100,000 queries per workload and 15 repetitions: about 40 minutes, plus the large
run.

## 4. The false-positive study

```sh
./bench.sh fprate results/fp900     # 871,000 keys in a 900,000-key table; about 30 minutes
```

This runs `fprate` for 16, 24, and 32-bit fingerprints on both hash paths, three seeds
each, probing 1×10⁹, 3×10⁹, and 3×10¹⁰ unknown barcodes per seed on all cores. It needs
no `sudo` or pinning. Options (see `src/bin/fprate.rs`): `--count` (871000),
`--capacity` (900000), `--bits`, `--hash`, `--seeds` (0,1,2), `--probes`, `--threads`,
and `--output`. Each output file holds per-seed and pooled counts, Wilson intervals, and
the textbook estimate.

## 5. Reading the results

Each timing run writes a `results.json` with its configuration, environment (CPU, kernel,
governor, turbo flag, allowed CPUs, rustc, dependency versions), a hash of the source, a
hash of the dataset, correctness counts, raw per-batch records, summaries, and latency
percentiles. To print the tables used in these docs:

```sh
cargo run --release --bin report -- results/run1 results/large1
```

Results and build output are not kept in the repository, so run the steps above to
recreate them.

## 6. Notes

The same seed recreates the same barcodes under the same code and dependency versions.
Compare timing ratios across machines, not nanoseconds.

## Troubleshooting

- **"output directory already exists":** choose a new output name.
- **`setup` refuses to run:** `.host-state` is left over from an earlier setup. Run `./bench.sh restore` first.
- **Noisy timings:** check the governor and allowed CPUs recorded in `results.json`, close other work, and rerun.
- **`taskset: failed to set affinity`:** the chosen CPU is offline or outside your allowed set.
