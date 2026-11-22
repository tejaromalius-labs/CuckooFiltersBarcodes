# How the filter works

This guide follows one barcode through the filter, then explains how inserts, sizing,
and deletes work. If you only want the big picture, read sections 1 and 2.

## 1. A lookup, step by step

The table stores a short fingerprint for each barcode. Fingerprints can be 16, 24, 32 or
64 bits wide. A slot holding 0 is empty, so fingerprints run from 1 to 2^bits − 1. Four
neighboring slots make a bucket.

1. Turn the barcode's ASCII bytes into a 32-byte digest. (Which hash does this is covered in [Choosing the hash](hash-design.md).)
2. Hash the digest with seeded xxHash to get the fingerprint `fp` and the first bucket `i1`.
3. Hash the fingerprint to get an offset, and XOR it with `i1` to get the second bucket `i2`.
4. Look through the slots of those two buckets for `fp`. Stop at the first match.

```text
# Fingerprint (never zero, in [1, 2^bits - 1]); M = 2^bits - 1
bits <= 24: fp = (xxh32(digest, seed=0x5F)    % M) + 1
bits <= 32: fp = (xxh64(digest, seed=0x5F)    % M) + 1
bits == 64: fp = (xxh3_128(digest, seed=0x5F) % M) + 1   # folded: 2^64 = 1 (mod 2^64 - 1)

mask   = num_buckets - 1
i1     = xxh32(digest, seed=1) & mask
offset = xxh32(fp as bits/8 little-endian bytes, seed=0x77) & mask
i2     = i1 ^ offset
```

`tests/golden.txt` holds 320 reference vectors (fingerprint, `i1`, `i2` for 40 barcodes,
four widths, two table sizes) that the tests compare against, so the formulas can't drift
unnoticed. The two buckets can be the same one (when the offset happens to be 0, a
1-in-`num_buckets` chance), so a lookup compares at most eight slots.

### Why "found" only means "probably"

Two different barcodes can share a fingerprint and candidate buckets. The table keeps
neither the barcode nor its details, so it can't tell them apart. That is a false
positive. When you need certainty, confirm against an exact store.

### Why XOR lets items move

XOR has a handy property: applying the same offset twice returns you to where you
started.

```text
alternate = current ^ offset
current   = alternate ^ offset
```

Because the bucket count is a power of two, every XOR result lands inside the table. So a
stored fingerprint can hop to its other bucket knowing only its own value and where it
currently sits, never the original barcode.

## 2. Inserting, and sizing the table

1. Look in both candidate buckets for an empty slot; if there is one, store the fingerprint there.
2. If both are full, pick one of the two buckets and swap your fingerprint with a random resident.
3. Send the evicted fingerprint to its other bucket and look for space there.
4. Repeat until something finds a free slot, or 500 swaps have happened (the default limit).

Every swap is logged. If the insert fails, the log is replayed backwards, so the table
ends up exactly as it was and the item count is unchanged (`total_kicks` still counts the
effort). Each filter has its own seeded ChaCha8 random generator, so runs are
reproducible.

The number of buckets is the smallest power of two at or above
`capacity ÷ (4 × 0.95)`:

| Items | Slots | 16-bit | 24-bit | 32-bit | 64-bit | Load if full |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 900,000 (also used for 871,000) | 1,048,576 | 2 MiB | 3 MiB | 4 MiB | 8 MiB | 85.83% (83.07% at 871,000) |
| 20,000,000 | 33,554,432 | 64 MiB | 96 MiB | 128 MiB | 256 MiB | 59.60% |

Capacity is a sizing target, not a guarantee. Rounding up to a power of two means the
20-million row ends up well under-filled. Storage is a plain `u16`, `u32` or `u64` array
for 16, 32 and 64 bits, and genuine packed 3-byte slots for 24 bits (one spare byte at
the end lets reads load four bytes at once). Sizes are table payload only, not process
memory.

Inserting the same barcode over and over fills both buckets, and the ninth copy fails
(two buckets of four slots hold at most eight copies). The tests check this limit.

## 3. Deleting and updating safely

A delete finds the same fingerprint and buckets as a lookup, clears one matching slot,
and decrements the count. Check that the barcode is really in the catalog before you
delete: if you remove an absent barcode whose fingerprint collides with another item's,
you will erase that other item's entry, and the filter will then wrongly say it's
missing.

Lookups and deletes touch a fixed number of slots. Inserts can take many swaps or fail.
Nothing coordinates concurrent readers and writers, and there is no persistence or sync
protocol.

## 4. Where things live

| Path | What it does |
| --- | --- |
| `src/filter.rs` | The table (`Table16/24/32/64`), inserts with rollback, lookups, deletes |
| `src/digest.rs` | The two ways to turn a barcode into a digest (`blake3-xxh`, `xxh3`) |
| `src/baselines.rs` | SQLite export and read-only connections, and the sorted packed-array baseline |
| `src/dataset.rs` | Seeded, unique, valid synthetic EAN-13 barcodes |
| `src/stats.rs` | Wilson interval, median, MAD |
| `src/main.rs` | Workloads, correctness checks, timing, latency histograms, JSON output |
| `src/bin/report.rs` | Turns result files into the tables used in these docs |
| `src/bin/fprate.rs` | Counts false positives over billions of absent barcodes on all cores |
| `tests/filter.rs`, `tests/golden.txt` | Tests and reference vectors |
| `bench.sh` | Locks the host and runs the standard benchmark sets |

## 5. Limits

The filter lives in memory only. Persistence, snapshot loading, catalog sync, and
concurrent updates are not implemented, and the benchmark measures table payload
directly without writing a filter to disk.
