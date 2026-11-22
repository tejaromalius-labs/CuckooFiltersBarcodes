//! Partial-key cuckoo filter. The seeds and formulas are pinned by `tests/golden.txt`.
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use xxhash_rust::{xxh3::xxh3_128_with_seed, xxh32::xxh32, xxh64::xxh64};

pub const FINGERPRINT_SEED: u32 = 0x5F;
pub const ALT_HASH_SEED: u32 = 0x77;
const BUCKET_SEED: u32 = 1;

/// Slot storage for one fingerprint width. Zero marks an empty slot.
pub trait Table {
    const BITS: u32;
    fn new(slots: usize) -> Self;
    fn get(&self, i: usize) -> u64;
    fn set(&mut self, i: usize, v: u64);
    fn payload_bytes(&self) -> usize;
    fn slots(&self) -> usize;
}

macro_rules! native_table {
    ($name:ident, $t:ty, $bits:expr) => {
        pub struct $name(Vec<$t>);
        impl Table for $name {
            const BITS: u32 = $bits;
            fn new(slots: usize) -> Self {
                Self(vec![0; slots])
            }
            #[inline(always)]
            fn get(&self, i: usize) -> u64 {
                self.0[i] as u64
            }
            #[inline(always)]
            fn set(&mut self, i: usize, v: u64) {
                self.0[i] = v as $t;
            }
            fn payload_bytes(&self) -> usize {
                self.0.len() * std::mem::size_of::<$t>()
            }
            fn slots(&self) -> usize {
                self.0.len()
            }
        }
    };
}
native_table!(Table16, u16, 16);
native_table!(Table32, u32, 32);
native_table!(Table64, u64, 64);

/// Genuine 3-byte slots. One trailing pad byte lets reads load a u32 unconditionally.
pub struct Table24(Vec<u8>, usize);
impl Table for Table24 {
    const BITS: u32 = 24;
    fn new(slots: usize) -> Self {
        Self(vec![0; slots * 3 + 1], slots)
    }
    #[inline(always)]
    fn get(&self, i: usize) -> u64 {
        let o = i * 3;
        let w = u32::from_le_bytes([self.0[o], self.0[o + 1], self.0[o + 2], self.0[o + 3]]);
        (w & 0x00FF_FFFF) as u64
    }
    #[inline(always)]
    fn set(&mut self, i: usize, v: u64) {
        let o = i * 3;
        self.0[o] = v as u8;
        self.0[o + 1] = (v >> 8) as u8;
        self.0[o + 2] = (v >> 16) as u8;
    }
    fn payload_bytes(&self) -> usize {
        self.1 * 3
    }
    fn slots(&self) -> usize {
        self.1
    }
}

pub struct CuckooFilter<T: Table> {
    pub table: T,
    pub b: usize,
    pub max_kicks: u32,
    pub num_buckets: usize,
    mask: u64,
    pub count: usize,
    pub total_kicks: u64,
    rng: ChaCha8Rng,
    swaps: Vec<(usize, u64)>,
}

impl<T: Table> CuckooFilter<T> {
    const MAX_FP: u64 = if T::BITS == 64 { u64::MAX } else { (1u64 << T::BITS) - 1 };
    const FP_BYTES: usize = (T::BITS / 8) as usize;

    pub fn new(capacity: usize, slots_per_bucket: usize, max_kicks: u32, seed: u64) -> Self {
        assert!(capacity > 0 && slots_per_bucket > 0);
        let raw = (capacity as f64 / (slots_per_bucket as f64 * 0.95)).ceil() as usize;
        let num_buckets = raw.next_power_of_two();
        Self {
            table: T::new(num_buckets * slots_per_bucket),
            b: slots_per_bucket,
            max_kicks,
            num_buckets,
            mask: (num_buckets - 1) as u64,
            count: 0,
            total_kicks: 0,
            rng: ChaCha8Rng::seed_from_u64(seed),
            swaps: Vec::new(),
        }
    }

    pub fn total_slots(&self) -> usize {
        self.table.slots()
    }

    /// Nonzero fingerprint in `[1, MAX_FP]`, fixed by the reference vectors in `tests/golden.txt`.
    #[inline(always)]
    pub fn fingerprint(digest: &[u8]) -> u64 {
        let r = if T::BITS <= 24 {
            xxh32(digest, FINGERPRINT_SEED) as u64 % Self::MAX_FP
        } else if T::BITS <= 32 {
            xxh64(digest, FINGERPRINT_SEED as u64) % Self::MAX_FP
        } else {
            // h mod (2^64 - 1): 2^64 == 1, so fold the halves.
            let h = xxh3_128_with_seed(digest, FINGERPRINT_SEED as u64);
            let mut s = (h >> 64) + (h & u64::MAX as u128);
            while s >= u64::MAX as u128 {
                s -= u64::MAX as u128;
            }
            s as u64
        };
        r + 1
    }

    #[inline(always)]
    fn hash_fp(&self, fp: u64) -> u64 {
        let bytes = fp.to_le_bytes();
        xxh32(&bytes[..Self::FP_BYTES], ALT_HASH_SEED) as u64 & self.mask
    }

    /// Primary and alternate bucket for a digest and its fingerprint.
    #[inline(always)]
    pub fn indices(&self, digest: &[u8], fp: u64) -> (usize, usize) {
        let i1 = xxh32(digest, BUCKET_SEED) as u64 & self.mask;
        (i1 as usize, ((i1 ^ self.hash_fp(fp)) & self.mask) as usize)
    }

    fn find_empty(&self, bucket: usize) -> Option<usize> {
        let base = bucket * self.b;
        (base..base + self.b).find(|&i| self.table.get(i) == 0)
    }

    pub fn insert(&mut self, digest: &[u8]) -> bool {
        let fp = Self::fingerprint(digest);
        let (i1, i2) = self.indices(digest, fp);
        for i in [i1, i2] {
            if let Some(slot) = self.find_empty(i) {
                self.table.set(slot, fp);
                self.count += 1;
                return true;
            }
        }
        let mut cur_i = if self.rng.random::<bool>() { i1 } else { i2 };
        let mut cur_fp = fp;
        self.swaps.clear();
        for _ in 0..self.max_kicks {
            self.total_kicks += 1;
            let idx = cur_i * self.b + self.rng.random_range(0..self.b);
            let displaced = self.table.get(idx);
            self.swaps.push((idx, displaced));
            self.table.set(idx, cur_fp);
            cur_fp = displaced;
            cur_i = ((cur_i as u64 ^ self.hash_fp(cur_fp)) & self.mask) as usize;
            if let Some(slot) = self.find_empty(cur_i) {
                self.table.set(slot, cur_fp);
                self.count += 1;
                return true;
            }
        }
        // Reverse every swap, including repeated visits to the same slot.
        for &(idx, previous) in self.swaps.iter().rev() {
            self.table.set(idx, previous);
        }
        false
    }

    #[inline(always)]
    pub fn contains(&self, digest: &[u8]) -> bool {
        let fp = Self::fingerprint(digest);
        let (i1, i2) = self.indices(digest, fp);
        for bucket in [i1, i2] {
            let base = bucket * self.b;
            for off in 0..self.b {
                if self.table.get(base + off) == fp {
                    return true;
                }
            }
        }
        false
    }

    /// Caller must establish exact membership before deleting.
    pub fn delete_digest(&mut self, digest: &[u8]) -> bool {
        let fp = Self::fingerprint(digest);
        let (i1, i2) = self.indices(digest, fp);
        for bucket in [i1, i2] {
            let base = bucket * self.b;
            for off in 0..self.b {
                if self.table.get(base + off) == fp {
                    self.table.set(base + off, 0);
                    self.count -= 1;
                    return true;
                }
            }
        }
        false
    }

    pub fn snapshot(&self) -> Vec<u64> {
        (0..self.total_slots()).map(|i| self.table.get(i)).collect()
    }
}
