fn main() {
    let out = std::process::Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .arg("-V")
        .output()
        .expect("rustc -V");
    println!("cargo:rustc-env=RUSTC_VERSION={}", String::from_utf8_lossy(&out.stdout).trim());
}
