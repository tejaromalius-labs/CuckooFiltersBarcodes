use cuckoo_bench::dataset::generate_barcodes;
use cuckoo_bench::filter::*;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn check_golden<T: Table>(bits: u32) {
    let text = std::fs::read_to_string("tests/golden.txt").unwrap();
    let mut seen = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        if f[2].parse::<u32>().unwrap() != bits {
            continue;
        }
        let digest = hex(f[1]);
        assert_eq!(blake3::hash(f[0].as_bytes()).as_bytes()[..], digest[..], "blake3 {}", f[0]);
        let cap: usize = f[3].parse().unwrap();
        let cf = CuckooFilter::<T>::new(cap, 4, 500, 0);
        let fp = CuckooFilter::<T>::fingerprint(&digest);
        let (i1, i2) = cf.indices(&digest, fp);
        assert_eq!(fp, f[4].parse::<u64>().unwrap(), "fp {} bits={bits}", f[0]);
        assert_eq!(i1, f[5].parse::<usize>().unwrap(), "i1 {}", f[0]);
        assert_eq!(i2, f[6].parse::<usize>().unwrap(), "i2 {}", f[0]);
        seen += 1;
    }
    assert_eq!(seen, 80);
}

#[test]
fn golden_vectors_all_widths() {
    check_golden::<Table16>(16);
    check_golden::<Table24>(24);
    check_golden::<Table32>(32);
    check_golden::<Table64>(64);
}

fn digests(n: usize, seed: u64) -> Vec<[u8; 32]> {
    generate_barcodes(n, seed).iter().map(|k| *blake3::hash(k).as_bytes()).collect()
}

fn roundtrip<T: Table>() {
    let keys = digests(1000, 4);
    let mut cf = CuckooFilter::<T>::new(1000, 4, 500, 4);
    assert!(keys.iter().all(|k| cf.insert(k)));
    assert!(keys.iter().all(|k| cf.contains(k)));
    for k in keys.iter().step_by(2) {
        assert!(cf.delete_digest(k));
    }
    assert_eq!(cf.count, 500);
    assert!(keys.iter().skip(1).step_by(2).all(|k| cf.contains(k)));
    assert_eq!(cf.table.payload_bytes(), cf.total_slots() * (T::BITS / 8) as usize);
}

#[test]
fn membership_and_delete_all_widths() {
    roundtrip::<Table16>();
    roundtrip::<Table24>();
    roundtrip::<Table32>();
    roundtrip::<Table64>();
}

fn rollback<T: Table>() {
    let mut cf = CuckooFilter::<T>::new(8, 4, 500, 1);
    let keys = digests(100, 9);
    let mut accepted = vec![];
    for k in &keys {
        let (before, count) = (cf.snapshot(), cf.count);
        if cf.insert(k) {
            accepted.push(*k);
        } else {
            assert_eq!(cf.snapshot(), before);
            assert_eq!(cf.count, count);
            assert!(accepted.iter().all(|a| cf.contains(a)));
            return;
        }
    }
    panic!("expected full-filter failure");
}

#[test]
fn failed_insert_restores_every_slot() {
    rollback::<Table16>();
    rollback::<Table24>();
    rollback::<Table32>();
    rollback::<Table64>();
}

#[test]
fn duplicate_cap_is_eight() {
    let mut cf = CuckooFilter::<Table16>::new(1000, 4, 500, 1);
    let d = blake3::hash(b"4006381333931");
    let mut n = 0;
    while cf.insert(d.as_bytes()) {
        n += 1;
    }
    assert_eq!(n, 8);
}

#[test]
fn seed_is_reproducible() {
    let keys = digests(100, 7);
    let build = || {
        let mut cf = CuckooFilter::<Table16>::new(100, 4, 500, 7);
        keys.iter().for_each(|k| assert!(cf.insert(k)));
        cf.snapshot()
    };
    assert_eq!(build(), build());
}

#[test]
fn barcodes_are_valid_ean13() {
    let keys = generate_barcodes(1000, 2);
    assert_eq!(keys.len(), keys.iter().collect::<std::collections::HashSet<_>>().len());
    for k in keys {
        let sum: u32 = k.iter().enumerate().map(|(i, d)| (d - b'0') as u32 * if i % 2 == 0 { 1 } else { 3 }).sum();
        assert_eq!(sum % 10, 0);
    }
}
