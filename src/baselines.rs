//! Exact baselines: SQLite (two schemas) and a sorted packed array.
use crate::dataset::{parse, Barcode};
use rusqlite::{Connection, OpenFlags};
use std::{fs::OpenOptions, path::Path};

/// Exact in-memory membership: EAN-13 values fit in 44 bits, stored as sorted 6-byte LE integers.
pub struct SortedArray {
    data: Vec<u8>,
}

impl SortedArray {
    pub fn build(keys: &[Barcode]) -> Self {
        let mut v: Vec<u64> = keys.iter().map(parse).collect();
        v.sort_unstable();
        let mut data = Vec::with_capacity(v.len() * 6);
        for x in v {
            data.extend_from_slice(&x.to_le_bytes()[..6]);
        }
        Self { data }
    }

    pub fn payload_bytes(&self) -> usize {
        self.data.len()
    }

    #[inline(always)]
    fn at(&self, i: usize) -> u64 {
        let o = i * 6;
        let d = &self.data[o..o + 6];
        u64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], 0, 0])
    }

    #[inline(always)]
    pub fn contains(&self, code: &Barcode) -> bool {
        let key = parse(code);
        let (mut lo, mut hi) = (0usize, self.data.len() / 6);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let v = self.at(mid);
            if v == key {
                return true;
            }
            if v < key { lo = mid + 1 } else { hi = mid }
        }
        false
    }
}

/// Builds the SQLite baseline; refuses to replace an existing file.
pub fn export_sqlite(keys: &[Barcode], path: &Path, without_rowid: bool) -> rusqlite::Result<u64> {
    OpenOptions::new().write(true).create_new(true).open(path).expect("database file already exists");
    let mut con = Connection::open(path)?;
    let suffix = if without_rowid { " WITHOUT ROWID" } else { "" };
    con.execute(&format!("CREATE TABLE barcodes (barcode TEXT PRIMARY KEY){suffix}"), [])?;
    let tx = con.transaction()?;
    {
        let mut stmt = tx.prepare("INSERT INTO barcodes VALUES (?1)")?;
        for k in keys {
            stmt.execute([std::str::from_utf8(k).unwrap()])?;
        }
    }
    tx.commit()?;
    drop(con);
    Ok(std::fs::metadata(path).unwrap().len())
}

/// Read-only connection: 2 MiB page cache, memory mapping disabled.
pub fn open_readonly(path: &Path) -> rusqlite::Result<Connection> {
    let con = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    con.pragma_update(None, "cache_size", -2048)?;
    con.pragma_update(None, "mmap_size", 0)?;
    Ok(con)
}
