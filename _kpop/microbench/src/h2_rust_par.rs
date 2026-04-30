// H2: Falsify the comment in src/rust_parsing.rs that claims syn::File can't
// be parsed in parallel. We will:
//   (a) parse all .rs files single-threaded and time it,
//   (b) parse all .rs files via rayon::par_iter, returning a small `Send`
//       summary (item count + statement count) per file. The syn::File never
//       crosses thread boundaries.
// Hypothesis is FALSIFIED if (a) <= (b), or if (b) doesn't compile.

use rayon::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Default, Clone, Copy)]
struct Summary {
    items: usize,
    bytes: usize,
}

fn parse_one(path: &PathBuf) -> Option<Summary> {
    let src = std::fs::read_to_string(path).ok()?;
    let f: syn::File = syn::parse_file(&src).ok()?;
    Some(Summary { items: f.items.len(), bytes: src.len() })
}

fn collect_rs(root: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let p = entry.path();
        if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p.to_path_buf());
        }
    }
    out
}

fn main() {
    let root = std::env::args().nth(1).expect("pass root");
    let paths = collect_rs(&root);
    eprintln!("rs_files={}", paths.len());

    // Warmup once (also OS file cache warmup)
    let warm: usize = paths.iter().filter_map(parse_one).map(|s| s.items).sum();
    eprintln!("warmup_items={}", warm);

    for trial in 0..3 {
        // Serial
        let t = Instant::now();
        let mut total_items = 0usize;
        let mut total_bytes = 0usize;
        for p in &paths {
            if let Some(s) = parse_one(p) {
                total_items += s.items;
                total_bytes += s.bytes;
            }
        }
        let d_ser = t.elapsed();

        // Parallel via par_iter (this proves syn parsing is parallelizable
        // when the syn::File is consumed locally before sending a Send result).
        let t = Instant::now();
        let summaries: Vec<Summary> = paths
            .par_iter()
            .filter_map(parse_one)
            .collect();
        let d_par = t.elapsed();
        let par_items: usize = summaries.iter().map(|s| s.items).sum();
        let par_bytes: usize = summaries.iter().map(|s| s.bytes).sum();

        println!(
            "trial={} ser={}ms par={}ms speedup={:.2}x  ser_items={} par_items={}  match={}",
            trial,
            d_ser.as_millis(),
            d_par.as_millis(),
            d_ser.as_secs_f64() / d_par.as_secs_f64(),
            total_items, par_items,
            (total_items == par_items) && (total_bytes == par_bytes)
        );
    }
}
