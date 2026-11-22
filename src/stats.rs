//! Uncertainty and summary statistics.

/// 95% Wilson score interval; probabilities, not percentages.
pub fn wilson_interval(successes: u64, trials: u64) -> [f64; 2] {
    let z = 1.959963984540054_f64;
    let n = trials as f64;
    let rate = successes as f64 / n;
    let denom = 1.0 + z * z / n;
    let center = (rate + z * z / (2.0 * n)) / denom;
    let half = z * (rate * (1.0 - rate) / n + z * z / (4.0 * n * n)).sqrt() / denom;
    [(center - half).max(0.0), (center + half).min(1.0)]
}

pub fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    if n % 2 == 1 { s[n / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 }
}

/// Median absolute deviation.
pub fn mad(v: &[f64]) -> f64 {
    let m = median(v);
    median(&v.iter().map(|x| (x - m).abs()).collect::<Vec<_>>())
}
