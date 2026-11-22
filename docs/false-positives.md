# False-positive study

A false positive happens when the filter says "probably in the catalog" for a barcode
that isn't. This guide measures how often that happens at the size one real customer
needs: 871,000 barcodes. Everything here is about counting, not timing.

## 1. What we measured

The table is sized for 900,000 keys (262,144 buckets, 1,048,576 slots) and we load
871,000, for a load of 83.07%. Then we probe it with billions of *distinct* barcodes that
we know are not in the catalog, and count how many the filter wrongly accepts.

- Probes follow `i → 1,000,000,007 × i + offset (mod 9×10¹¹)` across the 12-digit key space. That sequence never repeats a barcode, and any that happen to be in the catalog are skipped and not counted.
- All cores are used. Counts don't depend on timing, so the CPU doesn't need to be locked or pinned and no `sudo` is needed.
- Each configuration is built with three seeds (independent filters), then pooled.
- Probe counts per seed: 1×10⁹ (16-bit), 3×10⁹ (24-bit), 3×10¹⁰ (32-bit), scaled so each width sees enough events.
- For comparison we use the textbook estimate `2 × 4 × load ÷ (2^bits − 1)`.

## 2. Results

| Hash | Fingerprint | Absent probes | False positives | Rate | 95% Wilson interval | Textbook estimate | Observed ÷ estimate |
| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| BLAKE3 | 16-bit | 3.0×10⁹ | 309,043 | 1.030×10⁻⁴ | 1.03×10⁻⁴–1.03×10⁻⁴ | 1.014×10⁻⁴ | 1.02 |
| BLAKE3 | 24-bit | 9.0×10⁹ | 20,501 | 2.28×10⁻⁶ | 2.25×10⁻⁶–2.31×10⁻⁶ | 3.96×10⁻⁷ | 5.75 |
| BLAKE3 | 32-bit | 9.0×10¹⁰ | 162 | 1.80×10⁻⁹ | 1.54×10⁻⁹–2.10×10⁻⁹ | 1.55×10⁻⁹ | 1.16 |
| xxh3 | 16-bit | 3.0×10⁹ | 309,148 | 1.030×10⁻⁴ | 1.03×10⁻⁴–1.03×10⁻⁴ | 1.014×10⁻⁴ | 1.02 |
| xxh3 | 24-bit | 9.0×10⁹ | 20,064 | 2.23×10⁻⁶ | 2.20×10⁻⁶–2.26×10⁻⁶ | 3.96×10⁻⁷ | 5.63 |
| xxh3 | 32-bit | 9.0×10¹⁰ | 174 | 1.93×10⁻⁹ | 1.67×10⁻⁹–2.24×10⁻⁹ | 1.55×10⁻⁹ | 1.25 |

## 3. What it means for the customer

| Fingerprint | Table size | Roughly one false positive per… |
| --- | ---: | ---: |
| 16-bit | 2 MiB | 9,700 unknown-barcode lookups |
| 24-bit | 3 MiB | 440,000 |
| 32-bit | 4 MiB | 540 million |

1. **Going from 16 to 32 bits** costs 2 MiB and cuts false positives by almost five orders of magnitude.
2. **24-bit is the odd one out.** It is about 5.7× worse than the textbook estimate because the bucket index and fingerprint hashes are correlated at that width (see [Choosing the hash](hash-design.md)). The three seeds agree within about 1%, so this comes from the hash design, not a lucky filter. It is also about 1,200× worse than 32-bit for only 1 MiB less memory.
3. **The hash path doesn't change the rate** within sampling error, so you can choose xxh3 or BLAKE3 on speed alone.
4. **32-bit is known to about ±15%.** With 162–174 events the count is noisy (per-seed BLAKE3 counts were 66, 39 and 57), so use the pooled figure. The pooled BLAKE3 interval contains the textbook estimate; the xxh3 interval starts just above it.
5. **Spread between independent builds is small** for 16-bit (about ±0.1%) and 24-bit (about ±1%).
6. **Insertion succeeded in every run.** The 900,000-key timing runs reached 85.83% load without a failed insert, but we didn't test edits near the size limit.

## 4. Other false-positive counts in the project

The timing runs also count false positives, but over only 100,000 probes, which is why
they can't separate the wider fingerprints:

| Run | False positives per 100,000 | 95% interval |
| --- | ---: | --- |
| 16-bit BLAKE3, seeds 0 / 1 | 7 / 10 | 0.0034%–0.0144% / 0.0054%–0.0184% |
| 16-bit xxh3, seeds 0 / 1 | 8 / 15 | 0.0041%–0.0158% / 0.0091%–0.0247% |
| 24, 32, and 64-bit, both hashes, both seeds | 0 | 0.0000%–0.0038% |

Zero events in 100,000 probes only says the rate is below about 0.0038%. It can't rank
24, 32, and 64 bits, which is why this study uses billions of probes. No run ever missed
an inserted key.

One result is unexplained: the 20,000,000-key run (16-bit) counted 110 false positives per
million probes against about 73 expected at its 59.6% load. That is roughly a 4σ excess,
yet the 871,000-key study shows no excess at 16 bits, so we can't say why.

## 5. Caveats

- Barcodes are synthetic and uniformly random; a real catalog may cluster, though both hash paths scramble keys before use.
- Three seeds per configuration is a small number of independent builds.
- The hashes are unkeyed. An adversary who crafts barcodes could do worse than these rates.
- This measures the rate at one load (83.07%) and one table size. Different loads or sizes will change it.
