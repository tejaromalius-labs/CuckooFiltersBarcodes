# Cuckoo filters for warehouse handhelds

Can a handheld scanner keep a tiny, editable copy of its product catalog in memory
and say "that barcode isn't one of ours" without asking a database or the network?
This project tests that idea with a Cuckoo filter written in Rust, and measures how
fast it is, how much memory it needs, and how often it wrongly says "maybe".

It is a research prototype: everything here was measured on a desktop CPU, not on a
handheld.

## The idea in a minute

A Cuckoo filter stores a short fingerprint of each barcode instead of the barcode
itself. To check a scan, you look in two small buckets of memory. If the fingerprint
isn't there, the item is definitely not in the catalog. If it is there, the item is
*probably* in the catalog; once in a while a different barcode shares the fingerprint
(a "false positive"), so anything that must be certain still gets confirmed against a
real database. Unlike a Bloom filter, you can also remove items later.

## What we found

Measured on one pinned core of an i7-12700H at 2.3 GHz, with 900,000 barcodes loaded
(median time per lookup):

| Method | Barcode not in catalog | Barcode in catalog |
| --- | ---: | ---: |
| Filter, table search only (16-bit) | 39–49 ns | 85–86 ns |
| Filter, with BLAKE3 hashing | 188–190 ns | 205–210 ns |
| Filter, with xxh3 hashing | 44–45 ns | 83 ns |
| Sorted array of packed keys (exact) | ~290 ns | ~277 ns |
| SQLite | 5.6–5.9 µs | 5.6–5.9 µs |
| Filter (BLAKE3) in front of SQLite | 192–212 ns | ~6.04 µs |

- **Putting the filter in front of SQLite** made lookups for unknown barcodes about 22–30× faster and cost only 4–6% extra for known ones.
- **The hash is the expensive part.** BLAKE3 is about three quarters of a lookup; swapping in xxh3 makes the whole lookup roughly 4× faster with the same false-positive rate.
- **An exact sorted array is a serious rival.** It is 5.15 MiB, never wrong, and only 1.4–6× slower than the filter. The filter's real edge is that you can add and remove items in place.
- **For a customer with 871,000 barcodes,** the chance a random unknown barcode slips through is about 1 in 9,700 with 16-bit fingerprints, 1 in 440,000 with 24-bit, and 1 in 540 million with 32-bit (tables of 2, 3, and 4 MiB).

What we did *not* measure: handheld hardware, battery life, network savings, or how
the filter behaves while a catalog is being edited near its limit.

## Try it

```sh
cargo test --release                 # check the filter
cargo run --release -- --count 100000 --queries 20000 --repeats 3 --output results/smoke
./bench.sh fprate results/fp900      # false-positive study, about 30 minutes, no sudo
```

The [reproduction guide](docs/reproduction.md) covers the full, host-locked timing runs.

## Guide to the docs

| Read this | If you want to know |
| --- | --- |
| [Warehouse case study](docs/warehouse-case-study.md) | Why a handheld would want this, how a scan flows through it, and what a real deployment would still need. |
| [How the filter works](docs/architecture.md) | Fingerprints, the two candidate buckets, how items get moved around, table sizing, deleting safely, and where the code lives. |
| [Choosing the hash](docs/hash-design.md) | Why BLAKE3 vs xxh3 matters, why digests must be 32 bytes, and the one design weakness we found at 24 bits. |
| [How we measured](docs/methodology.md) | The workloads, the exact baselines, how the machine was locked down, and what the numbers can and can't tell you. |
| [Speed and space results](docs/results.md) | Timing tables, comparisons with SQLite and the sorted array, the 20-million-key run, and memory sizes. |
| [False-positive study](docs/false-positives.md) | How often each fingerprint width wrongly says "maybe", measured over billions of unknown barcodes at 871,000 keys. |
| [Reproducing the experiments](docs/reproduction.md) | Build, test, lock the CPU, run the benchmarks, and read the output. |

Generated results are local only and ignored by version control; results from earlier
runs aren't stored in the repository, so rerun the commands to recreate them.
