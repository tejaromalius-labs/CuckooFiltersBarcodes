//! Prints markdown tables from one or more result directories (each holding `<run>/results.json`).
//!   cargo run --release --bin report -- results/run1 [results/large1 ...]
use serde_json::Value;
use std::path::PathBuf;

const METHODS: [&str; 7] = [
    "filter_lookup",
    "filter_hash_lookup",
    "sorted_array",
    "sqlite_rowid",
    "sqlite_without_rowid",
    "filter_plus_sqlite_rowid",
    "filter_plus_sqlite_without_rowid",
];

fn median(r: &Value, workload: &str, method: &str) -> Option<f64> {
    r["summary"].as_array()?.iter().find(|s| s["workload"] == workload && s["method"] == method)?["median_ns_per_query"].as_f64()
}

fn mad(r: &Value, workload: &str, method: &str) -> Option<f64> {
    r["summary"].as_array()?.iter().find(|s| s["workload"] == workload && s["method"] == method)?["mad_ns_per_query"].as_f64()
}

fn main() {
    let mut runs: Vec<(String, Value)> = vec![];
    for root in std::env::args().skip(1).map(PathBuf::from) {
        let mut dirs: Vec<_> = std::fs::read_dir(&root).expect("result directory").map(|e| e.unwrap().path()).collect();
        dirs.sort();
        for d in dirs {
            if let Ok(text) = std::fs::read_to_string(d.join("results.json")) {
                runs.push((d.file_name().unwrap().to_string_lossy().into(), serde_json::from_str(&text).unwrap()));
            }
        }
    }

    println!("### Setup and correctness\n");
    println!("| Run | Hash | Bits | Seed | Filter MiB | Sorted array MiB | Load | False positives | 95% Wilson interval |");
    println!("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
    for (name, r) in &runs {
        let (c, b, k) = (&r["configuration"], &r["build"], &r["correctness"]);
        let w = k["wilson_95_interval"].as_array().unwrap();
        println!("| {name} | {} | {} | {} | {:.2} | {:.2} | {:.4} | {} / {} | {:.4}%–{:.4}% |",
                 c["hash_path"].as_str().unwrap(), c["fingerprint_bits"], c["seed"],
                 b["filter"]["table_bytes"].as_f64().unwrap() / 1048576.0, b["sorted_array"]["table_bytes"].as_f64().unwrap() / 1048576.0,
                 b["filter"]["load_factor"].as_f64().unwrap(), k["false_positives"], k["negative_queries"],
                 w[0].as_f64().unwrap() * 100.0, w[1].as_f64().unwrap() * 100.0);
    }

    println!("\n### Median ns per query (± MAD)\n");
    for workload in ["positive", "negative", "mixed"] {
        println!("**{workload}**\n");
        println!("| Run | {} |", METHODS.join(" | "));
        println!("| --- |{}", " ---: |".repeat(METHODS.len()));
        for (name, r) in &runs {
            let cells: Vec<String> = METHODS.iter().map(|m| match median(r, workload, m) {
                Some(v) => format!("{v:.1} ± {:.1}", mad(r, workload, m).unwrap()),
                None => "–".into(),
            }).collect();
            println!("| {name} | {} |", cells.join(" | "));
        }
        println!();
    }

    println!("### Ratios (from medians)\n");
    println!("| Run | Absent: SQLite ÷ filter+SQLite | Present: filter+SQLite vs SQLite | Mixed: filter+SQLite vs SQLite | Absent: sorted array ÷ filter (hash incl.) |");
    println!("| --- | ---: | ---: | ---: | ---: |");
    for (name, r) in &runs {
        let (Some(sq_n), Some(fs_n)) = (median(r, "negative", "sqlite_rowid"), median(r, "negative", "filter_plus_sqlite_rowid")) else { continue };
        let pos = median(r, "positive", "filter_plus_sqlite_rowid").unwrap() / median(r, "positive", "sqlite_rowid").unwrap() - 1.0;
        let mix = median(r, "mixed", "filter_plus_sqlite_rowid").unwrap() / median(r, "mixed", "sqlite_rowid").unwrap() - 1.0;
        let arr = median(r, "negative", "sorted_array").unwrap() / median(r, "negative", "filter_hash_lookup").unwrap();
        println!("| {name} | {:.1}× | {:+.1}% | {:+.1}% | {:.1}× |", sq_n / fs_n, pos * 100.0, mix * 100.0, arr);
    }
}
