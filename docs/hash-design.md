# Choosing the hash

How a barcode becomes a fingerprint and a bucket matters more for speed than anything
else in this project, and it hides one design weakness. This guide covers both.

## 1. Two steps

1. **Prehash:** turn the 13-byte barcode into a 32-byte digest.
2. **Derivation:** the filter hashes that digest with seeded xxHash to get the fingerprint, the first bucket, and (from the fingerprint) the second bucket. See [How the filter works](architecture.md).

The benchmark offers two prehash options, selected with `--hash`:

| Option | What it does | Cost (from the results) |
| --- | --- | --- |
| `blake3-xxh` | BLAKE3 of the barcode, 32 bytes | About 150 ns of a ~190 ns lookup |
| `xxh3` | Two XXH3-128 hashes (seeds 0 and 1) joined into 32 bytes | A few ns; the full lookup is ~45 ns |

Swapping BLAKE3 for xxh3 made a lookup roughly 4× faster, and the false-positive rate
did not change within sampling error ([false-positive study](false-positives.md)). If
you aren't defending against someone who crafts barcodes, xxh3 is the cheaper choice.
Neither option is keyed: both use fixed seeds, so neither resists an attacker who wants
to overload particular buckets.

## 2. Always use a 32-byte digest

We first tried an `xxh3` variant that fed the filter a 16-byte digest. It produced 3–4
false positives per 100,000 absent barcodes at 24 bits, where about 0.04 was expected,
roughly 100× too many. The cause is that the filter derives its bucket and fingerprint
by running `xxh32` over the same digest with two different seeds, and on short inputs
those two outputs aren't independent. We discarded that variant and gave `xxh3` a
32-byte digest, and the rule is now that the digest must be 32 bytes.

## 3. A weakness at 24 bits

Even with 32-byte digests, the two outputs are not fully independent at 24 bits. Measured
at 871,000 keys, compared with the textbook estimate `2 × 4 × load ÷ (2^bits − 1)`:

| Fingerprint | Observed ÷ estimate (BLAKE3) | (xxh3) |
| --- | ---: | ---: |
| 16-bit | 1.02 | 1.02 |
| 24-bit | 5.75 | 5.63 |
| 32-bit | 1.16 | 1.25 |

So 24-bit fingerprints produce about 5.7× more false positives than the estimate, and
16-bit and 32-bit match it. We think we know why, though we haven't tested it: a 24-bit
fingerprint is a 32-bit `xxh32` output reduced mod 2²⁴−1, which keeps its low bits, while
the bucket index uses the low bits of another `xxh32` of the same digest. The 16-bit
reduction (mod 2¹⁶−1) folds the two halves of the output together, and 32-bit
fingerprints come from `xxh64`, a different function.

A cleaner design would take the bucket index and fingerprint from separate bits of one
wide hash. This project keeps the original design so that the measurements describe it.
If you adopt 24-bit fingerprints, use the observed rate in the false-positive study, not
the textbook estimate.
