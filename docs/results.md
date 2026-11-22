# Speed and space results

Here is what the Rust benchmark measured on one desktop core. In short: every run kept
every catalog key findable and all exact methods returned correct answers. Putting the
filter in front of SQLite made lookups for unknown barcodes dramatically faster, known
barcodes paid only a small extra cost, and an exact sorted array turned out to be a strong,
if slower, alternative. These are observations from one machine, not guarantees for other
hardware or workloads. For false-positive rates see the
[false-positive study](false-positives.md).

## 1. The setup

Each standard run loaded 900,000 synthetic EAN-13 barcodes and timed 100,000 queries in
each of three workloads (unknown barcodes, known barcodes, a 50/50 mix), with 15
repetitions. That's seeds 0 and 1, four fingerprint widths, and two prehash paths: 16
runs. A separate 20,000,000-key run, without SQLite, checks a table much larger than the
CPU cache.

| Detail | Value |
| --- | --- |
| CPU | 12th Gen Intel Core i7-12700H, one P-core (CPU 4), hyper-threading sibling offline |
| Clock | 2.3 GHz (base; turbo off), governor `performance` |
| Kernel | Linux 7.0.0-42-generic |
| Compiler | rustc 1.98.1, release build, fat LTO, `target-cpu=native` |
| SQLite | 3.54.1 (bundled with rusqlite 0.42.0), 2 MiB page cache, mmap off |
| Hash crates | blake3 1.8.7, xxhash-rust 0.8.19 |
| Cache regime | Workloads warmed before timing; OS cache uncontrolled |

Every number below is a median of 15 batch averages, in nanoseconds per query. The
typical spread (MAD) is 1–3% of the median, but different seeds differ by more than that,
so seeds are shown side by side. The [reproduction guide](reproduction.md) shows how to
regenerate any table.

## 2. Lookup times

Each cell reads `table search only / including hashing`, seed 0 first, then seed 1,
separated by a semicolon. The first number searches with a digest computed in advance; the
second is the realistic cost.

### With the BLAKE3 prehash (`--hash blake3-xxh`)

| Fingerprint | Unknown barcodes | Known barcodes | 50/50 mix |
| --- | ---: | ---: | ---: |
| 16-bit | 39 / 190; 49 / 187 | 86 / 205; 85 / 210 | 62 / 208; 63 / 196 |
| 24-bit | 60 / 210; 68 / 197 | 91 / 209; 89 / 208 | 76 / 212; 75 / 204 |
| 32-bit | 43 / 196; 54 / 198 | 95 / 237; 92 / 238 | 70 / 227; 73 / 218 |
| 64-bit | 42 / 193; 43 / 208 | 99 / 250; 97 / 259 | 77 / 256; 76 / 251 |

### With the xxh3 prehash (`--hash xxh3`)

| Fingerprint | Unknown barcodes | Known barcodes | 50/50 mix |
| --- | ---: | ---: | ---: |
| 16-bit | 39 / 44; 46 / 45 | 86 / 83; 85 / 83 | 64 / 69; 67 / 72 |
| 24-bit | 57 / 59; 67 / 63 | 84 / 81; 90 / 90 | 70 / 72; 78 / 82 |
| 32-bit | 44 / 49; 70 / 63 | 96 / 112; 97 / 112 | 73 / 95; 79 / 92 |
| 64-bit | 48 / 51; 71 / 54 | 99 / 103; 98 / 104 | 76 / 91; 84 / 95 |

How repeatable are these? Repeating one configuration (16-bit, seed 0, BLAKE3) gave
medians within 1–1.6% for SQLite, the sorted array, and the table search. Rows that
include hashing moved by up to 3.4% (6.4% for the mixed workload), so treat those as
good to about ±5%.

What the tables show:

1. **Hashing dominates.** About 150 ns of a ~190 ns BLAKE3 lookup is the prehash. With xxh3, a 16-bit lookup for an unknown barcode takes 44–45 ns.
2. **Fingerprint width barely changes speed.** For known barcodes, 16-bit takes 86 ns and 64-bit 99 ns, probably because the larger tables (2 vs 8 MiB) no longer fit in the core's L2 cache. The packed 24-bit table costs about 5 ns more than 16-bit for known barcodes and about 20 ns more for unknown ones.
3. **Known barcodes cost about twice as much as unknown ones** for the table search. An unknown barcode always scans all eight slots in a predictable pattern, while a known one stops at an unpredictable spot. Branch misprediction is the likely reason; we haven't tested it.
4. **Seeds matter as much as widths.** The 24-bit search for unknown barcodes took 57 ns with seed 0 and 67 ns with seed 1, because the table layout depends on the seed. One seed would understate the spread.

## 3. Comparison with SQLite and the exact sorted array

Cells are seed 0 / seed 1.

### 16-bit, BLAKE3 prehash: exact baselines and the combined path

| Method | Unknown barcodes | Known barcodes | 50/50 mix |
| --- | ---: | ---: | ---: |
| Sorted packed array | 290 / 291 | 276 / 279 | 283 / 287 |
| SQLite | 5771 / 5721 | 5802 / 5750 | 5772 / 5719 |
| SQLite WITHOUT ROWID | 5773 / 5653 | 5729 / 5662 | 5742 / 5667 |
| Filter + SQLite | 212 / 191 | 6038 / 6045 | 3155 / 3117 |
| Filter + SQLite WITHOUT ROWID | 191 / 207 | 6028 / 5942 | 3119 / 3087 |

Ratios of medians:

| Comparison | BLAKE3 prehash | xxh3 prehash |
| --- | ---: | ---: |
| Unknown barcodes: SQLite ÷ (filter + SQLite) | 21.7–29.9× faster | 65–127× faster |
| Known barcodes: (filter + SQLite) vs SQLite | +4.1% to +6.0% time | +2.3% to +3.1% time |
| 50/50 mix: (filter + SQLite) vs SQLite | 44–46% less time | 47–48% less time |
| Unknown barcodes: sorted array ÷ filter (hashing included) | 1.4–1.6× slower | 4.6–6.6× slower |

The sorted array is a fair rival for this kind of key. An EAN-13 value fits in 44 bits,
so 900,000 keys take 5.15 MiB (6 bytes each) and the array is never wrong. The filter is
1.4–1.6× faster with BLAKE3 and 4.6–6.6× faster with xxh3, and uses 2–8 MiB, but the array
cannot be edited in place. The xxh3 ratios against SQLite (65–127×) are limited by
SQLite's ~5.8 µs, not by the filter, so read them as "very large", not as precise.

## 4. A larger table: 20,000,000 keys

No SQLite here; the aim is to see a table much bigger than the cache. Seed 0 only.

| Fingerprint | Filter | Sorted array | Load | Unknown: search / with hashing | Known | Sorted array (unknown) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 16-bit | 64.00 MiB | 114.44 MiB | 0.596 | 79 / 295 ns | 108 / 289 ns | 627 ns |
| 32-bit | 128.00 MiB | 114.44 MiB | 0.596 | 98 / 303 ns | 122 / 301 ns | 630 ns |

Binary search slows down the most (276 ns at 900,000 keys to 627 ns here), while the
filter's search goes from 39 to 79 ns (16-bit). Rounding up to a power of two leaves the
table at 59.6% load, so this overstates the memory a tightly sized filter would need.

## 5. Space

| Structure | 900,000 keys |
| --- | ---: |
| 16-bit filter | 2.00 MiB |
| 24-bit filter (packed) | 3.00 MiB |
| 32-bit filter | 4.00 MiB |
| 64-bit filter | 8.00 MiB |
| Sorted packed array (exact) | 5.15 MiB |
| SQLite WITHOUT ROWID | 17.11 MiB |
| SQLite ordinary | 38.99 MiB |

These are table and file sizes, not process memory. The SQLite files come from uniformly
random keys inserted in random order, which probably makes them larger than a real,
clustered catalog would be (untested). If you verify matches locally, you keep both the
filter and its exact store.

## 6. Per-lookup latency

The latency pass (serialized timestamp reads) for the 16-bit BLAKE3 table search, seed 0,
gave a median of 77–82 ns, 90th percentile 84–97 ns, 99th 106–210 ns, and 99.9th
278–344 ns across the three workloads. The sorted array gave a median of 276–282 ns and a
99.9th percentile of 576–632 ns. Maximums reached 4–8 µs from interrupts and scheduling,
even on a pinned core. These figures include serialization effects, so use them for the
tail, not for headline speed.

## 7. What this means for a handheld

The measurements support a compact, fast screen that cheaply rejects unknown barcodes in
front of a database. They also show the filter's edge over an exact sorted array is a
constant factor (about 1.5× with BLAKE3, up to 6× with xxh3) at this size, plus the
ability to edit in place, not a different class of algorithm. If a workflow accepts false
positives, the filter can stand alone.

They do not show that a handheld can drop SQLite, avoid network trips, shorten sync
pauses, save battery, or beat Bloom filters. Snapshot loading and catalog sync aren't
implemented, and these timings come from an x86 core at 2.3 GHz. Repeat the measurements
on the real handheld before deciding.
