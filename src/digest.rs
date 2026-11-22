//! How a barcode becomes the digest the filter consumes.
use crate::dataset::Barcode;
use xxhash_rust::xxh3::{xxh3_128, xxh3_128_with_seed};

#[derive(Clone, Copy)]
pub struct Digest {
    buf: [u8; 32],
    len: usize,
}
impl Digest {
    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

pub trait DigestFn: Sync {
    const NAME: &'static str;
    fn digest(k: &Barcode) -> Digest;
}

pub struct Blake3Xxh;
impl DigestFn for Blake3Xxh {
    const NAME: &'static str = "blake3-xxh";
    #[inline(always)]
    fn digest(k: &Barcode) -> Digest {
        Digest { buf: *blake3::hash(k).as_bytes(), len: 32 }
    }
}

/// Replaces only the BLAKE3 prehash; the filter's own xxHash derivations are unchanged.
/// The digest must be 32 bytes: xxh32 at different seeds is strongly correlated on 16-byte
/// inputs, which inflated false positives ~100x in a 16-byte variant.
pub struct Xxh3Direct;
impl DigestFn for Xxh3Direct {
    const NAME: &'static str = "xxh3";
    #[inline(always)]
    fn digest(k: &Barcode) -> Digest {
        let mut buf = [0u8; 32];
        buf[..16].copy_from_slice(&xxh3_128(k).to_le_bytes());
        buf[16..].copy_from_slice(&xxh3_128_with_seed(k, 1).to_le_bytes());
        Digest { buf, len: 32 }
    }
}
