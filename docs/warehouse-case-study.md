# Warehouse case study

Picture a worker scanning boxes during receiving, picking, or a cycle count. Each scan
needs a quick answer: does this barcode belong to our catalog? This guide explains where
a Cuckoo filter fits in that moment, and just as importantly, where it doesn't.

## 1. What decision does the handheld need to make?

Start by asking whether you need an approximate or an exact answer. That decides what
the filter can replace.

| What the handheld needs | What the filter can do |
| --- | --- |
| A "probably in the catalog" screen, with an accepted false-positive risk | Replace a local database that was only used for membership |
| An exact "yes/no" | Reject unknown barcodes locally, then confirm possible matches in an exact store |
| Stock counts, descriptions, bin locations | Screen first, then fetch the data from a database or service |
| Barcode decoding, format checks, damaged-label detection | Nothing. That's the scanner's job. |

The filter holds fingerprints, not barcodes or item data. For a customer with 871,000
barcodes in a table sized for 900,000, a random unknown barcode slips through about once
in 9,700 lookups with 16-bit fingerprints, once in 440,000 with 24-bit, and once in 540
million with 32-bit (see the [false-positive study](false-positives.md)). Your
application should decide what to do with those possible matches and should never
silently treat them as confirmed items.

## 2. Follow one scan

1. The scanner decodes the barcode and your app checks its format or checksum.
2. The app checks the filter, which looks in at most two buckets in RAM.
3. **Not found:** the barcode isn't in this filter's catalog. Reject it without a database or network call, as long as the filter is current enough for this decision.
4. **Found:** apply your workflow's rule. Accept the approximate answer if that risk is fine, or confirm against an exact store.
5. Fetch any item details the worker needs and give feedback.

Be careful with step 3. "Definitely absent" only holds for what has been applied to the
filter. A product added to the warehouse catalog after the last sync would be rejected
until the filter catches up, so the device needs a policy for stale or half-applied
updates.

## 3. Why a filter?

### It's quick

On one desktop core, searching the table took about 40–90 ns, and a lookup including
hashing took 45–260 ns depending on the hash and fingerprint width (see the
[results](results.md)). That is far below a millisecond. It doesn't include optical
decoding, screen updates, sound, or network calls, so measure scan-to-feedback time on
the real handheld before promising anything.

### It can change in place

Items can be added and removed without rebuilding. A static structure such as a minimal
perfect hash needs a rebuild when the catalog changes; none is implemented or timed
here. Lookups and deletes inspect a fixed number of slots. Inserts may have to shuffle
other fingerprints around, and we cap that at 500 moves. If an insert fails the filter is
restored to how it was, and you'd need a bigger filter. We tested update correctness but
not update speed under live scanning.

### It might skip a heavy database import

If the handheld only needs membership, you could build the filter elsewhere and ship it
as a ready-made snapshot instead of importing rows into SQLite on the device. This is a
proposal. There is no snapshot format, loader, or sync protocol here, and we haven't
measured sync pauses, battery, or flash wear.

### It's small

For 900,000 keys the table is 2 MiB (16-bit), 3 MiB (24-bit), 4 MiB (32-bit) or 8 MiB
(64-bit). For comparison, SQLite files were about 17 and 39 MiB, and an exact sorted array
of packed keys was 5.15 MiB. So against an exact in-memory structure the filter saves
about 1.3–2.6× at 16–32 bits, and nothing at 64 bits. If you keep both the filter and the
database, count both. Real transfer savings depend on compression, deltas, and metadata,
none of which we measured.

### It might be cache-friendly

A lookup touches two buckets, whereas a classic Bloom filter touches several scattered
bits. That is a reason to compare them, not proof of a win. We have no Bloom filter
baseline and no hardware cache counters, so we claim no cache advantage. See the
[original cuckoo-filter paper](https://www.cs.cmu.edu/~binfan/papers/conext14_cuckoofilter.pdf)
for background.

## 4. Applying catalog changes safely

If you build this into a device, treat this as a checklist:

1. Start from a known catalog version and a complete filter before allowing local "absent" decisions.
2. Receive versioned additions and removals from the authoritative catalog, tracking them so none are missed or applied twice.
3. Check every insert. If one fails, don't mark that update as applied; recover or resize.
4. Delete only keys you know are in the applied filter. A "found" answer isn't enough, because it might be a false positive.
5. Handle half-finished syncs explicitly so a partly updated filter never becomes the source of truth.

The core filter supports edits but provides no persistence, sync, or thread-safe
concurrent updates.

## 5. Where to go next

Reproduce the numbers with the [reproduction guide](reproduction.md), then read the
[results](results.md) and [false-positive study](false-positives.md). Before a real
rollout, add measurements on the target handheld (scan-to-feedback time, peak memory),
realistic hit rates, edits near the size limit, sync and loading tests, and any other
structures you'd consider. Keep those device findings separate from the desktop numbers
here.
