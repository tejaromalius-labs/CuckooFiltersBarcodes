//! Seeded synthetic EAN-13 barcodes (not assigned product identifiers).
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::HashSet;

pub type Barcode = [u8; 13];

pub fn check_digit(mut n: u64) -> u8 {
    let mut total = 0;
    let mut weight = 3;
    while n > 0 {
        total += weight * (n % 10);
        n /= 10;
        weight = 4 - weight;
    }
    ((10 - total % 10) % 10) as u8
}

/// 13-digit EAN-13 for a 12-digit base number.
pub fn encode(n: u64) -> Barcode {
    let mut code = [0u8; 13];
    let mut v = n * 10 + check_digit(n) as u64;
    for d in code.iter_mut().rev() {
        *d = b'0' + (v % 10) as u8;
        v /= 10;
    }
    code
}

/// `count` unique valid EAN-13 barcodes, 12-digit bases in `[10^11, 10^12)`.
pub fn generate_barcodes(count: usize, seed: u64) -> Vec<Barcode> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut seen = HashSet::with_capacity(count);
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let n = rng.random_range(10u64.pow(11)..10u64.pow(12));
        if !seen.insert(n) {
            continue;
        }
        let code = encode(n);
        out.push(code);
    }
    out
}

pub fn parse(code: &Barcode) -> u64 {
    code.iter().fold(0u64, |a, &d| a * 10 + (d - b'0') as u64)
}
