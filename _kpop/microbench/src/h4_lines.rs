// H4: Compare str.lines().count() vs memchr-based byte counting.
// Reads a tree of files from argv[1] (e.g. ~/repos/ruff), counts lines per
// file by both methods, prints aggregate timing and per-method total counts.

use std::path::PathBuf;
use std::time::Instant;

fn collect_paths(root: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        match p.extension().and_then(|e| e.to_str()) {
            Some("py") | Some("rs") => out.push(p.to_path_buf()),
            _ => {}
        }
    }
    out
}

fn lines_iter_count(s: &str) -> usize {
    s.lines().count()
}

// Match str::lines() semantics: a line is terminated by \n or \r\n; trailing
// empty after final newline is NOT counted.
fn memchr_lines(s: &str) -> usize {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return 0;
    }
    let nl = memchr::memchr_iter(b'\n', bytes).count();
    if *bytes.last().unwrap() == b'\n' {
        nl
    } else {
        nl + 1
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("pass a root path");
    let paths = collect_paths(&root);
    eprintln!("files={}", paths.len());

    // Read all sources up front so I/O isn't measured.
    let sources: Vec<String> = paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    eprintln!("read_ok={}, total_bytes={}", sources.len(),
              sources.iter().map(|s| s.len()).sum::<usize>());

    // Warmup
    let mut sink: usize = 0;
    for s in &sources { sink ^= lines_iter_count(s); }
    for s in &sources { sink ^= memchr_lines(s); }
    eprintln!("warmup_sink={}", sink);

    // 5 trials of each.
    for _ in 0..3 {
        let t = Instant::now();
        let mut total = 0usize;
        for s in &sources { total += lines_iter_count(s); }
        let d_iter = t.elapsed();

        let t = Instant::now();
        let mut total_mc = 0usize;
        for s in &sources { total_mc += memchr_lines(s); }
        let d_mc = t.elapsed();

        println!(
            "iter_total={} iter_us={}  memchr_total={} memchr_us={}  speedup={:.2}x  match={}",
            total,
            d_iter.as_micros(),
            total_mc,
            d_mc.as_micros(),
            d_iter.as_secs_f64() / d_mc.as_secs_f64(),
            total == total_mc
        );
    }
}
