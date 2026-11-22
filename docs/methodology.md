# How we measured

This guide explains exactly what the benchmark does, so you can judge the numbers fairly.
The question: how much can a native Cuckoo filter cut the cost of barcode membership
checks, on its own and in front of SQLite, compared with exact alternatives? The
[warehouse case study](warehouse-case-study.md) explains why that matters; this covers
the local lookup step only.

## 1. The same inputs for everyone

Barcodes are synthetic: unique 12-digit numbers drawn from a seeded ChaCha8 generator,
with an EAN-13 check digit added. They are not real product codes, and being uniformly
random they don't have the clustered prefixes a real catalog does.

For each run the benchmark:

1. Splits the barcodes into catalog keys and a separate set of unknown ones.
2. Draws "present" queries from the catalog keys, with replacement.
3. Uses the unknown ones as "absent" queries.
4. Builds a 50/50 mix, shuffled.
5. Feeds every method the same queries in the same order.

The seed and a SHA-256 of the barcode list are saved in the results.

## 2. Check correctness before timing

A run stops if an insert fails or any catalog key is missing from the filter. The sorted
array and both SQLite databases are checked against every catalog key and every absent
query before timing starts. During timing, the exact methods must return the expected hit
count in every batch, and the filter must find every present key.

## 3. The seven methods

| Name in the JSON | What happens for each query |
| --- | --- |
| `filter_lookup` | Search the filter table with a digest computed in advance (no hashing) |
| `filter_hash_lookup` | Hash the barcode, then search the table: the realistic cost |
| `sorted_array` | Parse the barcode to a number and binary-search 6-byte sorted entries (exact) |
| `sqlite_rowid` | `SELECT 1 … WHERE barcode = ?` on an ordinary `TEXT PRIMARY KEY` table |
| `sqlite_without_rowid` | The same query on a `WITHOUT ROWID` table |
| `filter_plus_sqlite_rowid` | Hash and check the filter; query the ordinary table only for possible matches |
| `filter_plus_sqlite_without_rowid` | The same, with the `WITHOUT ROWID` table |

SQLite uses long-lived read-only connections with prepared statements, a 2 MiB page
cache, and memory mapping turned off. `--hash` picks the prehash (`blake3-xxh` or `xxh3`;
see [Choosing the hash](hash-design.md)).

## 4. Taming the machine

Timing runs are pinned to one P-core of an i7-12700H with `taskset`. `bench.sh setup`
switches the CPU to the `performance` governor, turns turbo off, and takes that core's
hyper-threading sibling offline. The clock then holds at the 2.3 GHz base frequency, so
absolute times are slower than on a stock system. Compare ratios across machines, not
nanoseconds.

Then, for each workload:

1. Run two warmup passes per method.
2. Repeat 15 times: shuffle the method order, time a full batch, and consume every result so nothing is optimized away.
3. Save the elapsed time, query count, hit count, and position in the order for every batch.
4. Report the median, the MAD (median absolute deviation), the minimum, and the maximum.

Building the data, correctness checks, and JSON output are not timed. "Warm" means each
workload ran before timing; it doesn't mean SQLite's cache holds the database, and the
OS cache is left alone.

### Per-lookup latency

For the filter table search and the sorted array, a separate pass records every lookup's
cycle count with serialized timestamp-counter reads (five passes over each workload) and
reports the median, 90th, 99th, 99.9th percentile and the maximum. Serializing drains the
CPU pipeline, so these medians (about 77–82 ns for the filter) come out higher than the
batch medians (39–86 ns) even after subtracting the measured ~22 ns timer overhead. Use
them to see the shape of the tail, not the headline speed. SQLite has no per-call
histogram.

## 5. Space and build time

`table_bytes` is the filter's payload and `file_bytes` is each SQLite file's size.
Neither is process memory. If you deploy the filter and a database together, count both.
Build times are one-off observations with different boundaries (hashing and inserting for
the filter; file creation through commit for SQLite), so don't compare them directly.

## 6. False positives

False-positive rates have their own, much larger measurement. See the
[false-positive study](false-positives.md).

## 7. What a run can't tell you

It doesn't measure scanner responsiveness, battery, flash wear, network sync, Android
memory behavior, hardware cache counters, Bloom filters, minimal perfect hashes, or
update speed under load. Both SQLite baselines are local, so the gaps aren't avoided
network trips. And an x86 desktop core won't transfer to an ARM handheld in absolute
terms.

## Further reading

- [Fan et al., Cuckoo Filter: Practically Better Than Bloom (CoNEXT 2014)](https://www.cs.cmu.edu/~binfan/papers/conext14_cuckoofilter.pdf)
- [SQLite's WITHOUT ROWID guide](https://www.sqlite.org/withoutrowid.html)
