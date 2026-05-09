// H9: full sort vs select_nth_unstable for 4 percentiles + max.
// Tests at sizes representative of kiss stats workloads.

use std::time::Instant;

fn full_sort_pcts(values: &[usize]) -> (usize, usize, usize, usize, usize) {
    let mut s = values.to_vec();
    s.sort_unstable();
    let n = s.len();
    let pct = |p: f64| -> usize {
        let idx = ((n.saturating_sub(1) as f64) * p / 100.0).round() as usize;
        s[idx.min(n - 1)]
    };
    (pct(50.0), pct(90.0), pct(95.0), pct(99.0), *s.last().unwrap_or(&0))
}

fn select_pcts(values: &[usize]) -> (usize, usize, usize, usize, usize) {
    let mut s = values.to_vec();
    let n = s.len();
    if n == 0 { return (0, 0, 0, 0, 0); }
    let max = *s.iter().max().unwrap();
    let idx = |p: f64| ((n.saturating_sub(1) as f64) * p / 100.0).round() as usize;

    // Quantiles in ascending order; after each select_nth_unstable on the
    // upper sub-slice, everything to the right is >= the chosen value, and
    // we can keep narrowing.
    let i50 = idx(50.0);
    let (_, p50, rest) = s.select_nth_unstable(i50);
    let p50 = *p50;
    // For p90: it's the (idx(90)) overall, which lives in `rest` (the upper
    // portion after select). Its index within `rest` is idx(90) - i50 - 1.
    let i90 = idx(90.0);
    let i_in_rest = i90 - i50 - 1;
    let (_, p90, rest2) = rest.select_nth_unstable(i_in_rest);
    let p90 = *p90;
    let i95 = idx(95.0);
    let i_in_rest2 = i95 - i90 - 1;
    let (_, p95, rest3) = rest2.select_nth_unstable(i_in_rest2);
    let p95 = *p95;
    let i99 = idx(99.0);
    let i_in_rest3 = i99 - i95 - 1;
    let (_, p99, _rest4) = rest3.select_nth_unstable(i_in_rest3);
    let p99 = *p99;

    (p50, p90, p95, p99, max)
}

fn make(n: usize, seed: u64) -> Vec<usize> {
    // simple LCG, deterministic
    let mut x = seed;
    (0..n).map(|_| {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) as usize % 1000
    }).collect()
}

fn bench(n: usize) {
    let v = make(n, 42);
    // Warmup
    let _ = full_sort_pcts(&v);
    let _ = select_pcts(&v);

    let trials = if n < 100_000 { 200 } else if n < 1_000_000 { 20 } else { 5 };

    let t = Instant::now();
    let mut sink = (0, 0, 0, 0, 0);
    for _ in 0..trials { sink = full_sort_pcts(&v); }
    let dfs = t.elapsed();
    let res_full = sink;

    let t = Instant::now();
    let mut sink = (0, 0, 0, 0, 0);
    for _ in 0..trials { sink = select_pcts(&v); }
    let dse = t.elapsed();

    println!(
        "n={:>8}  trials={:>4}  full_sort={:>10}us  select={:>10}us  speedup={:.2}x  same={}",
        n, trials, dfs.as_micros(), dse.as_micros(),
        dfs.as_secs_f64() / dse.as_secs_f64(),
        res_full == sink
    );
}

fn main() {
    for &n in &[100usize, 1_000, 10_000, 100_000, 1_000_000, 10_000_000] {
        bench(n);
    }
}
