// H7: Compare Mutex<Vec<>> per-file walker vs per-thread accumulator walker.

use ignore::{ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn want(p: &std::path::Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()), Some("py") | Some("rs"))
}

fn walk_mutex(root: &str) -> Vec<PathBuf> {
    let results: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build_parallel()
        .run(|| {
            Box::new(|entry| {
                let Ok(entry) = entry else { return WalkState::Continue };
                if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                    return WalkState::Continue;
                }
                let p = entry.into_path();
                if want(&p) {
                    results.lock().unwrap().push(p);
                }
                WalkState::Continue
            })
        });
    results.into_inner().unwrap()
}

struct PtBuilder {
    chunks: Arc<Mutex<Vec<Vec<PathBuf>>>>,
}

struct PtVisitor {
    local: Vec<PathBuf>,
    chunks: Arc<Mutex<Vec<Vec<PathBuf>>>>,
}

impl<'s> ParallelVisitorBuilder<'s> for PtBuilder {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 's> {
        Box::new(PtVisitor { local: Vec::new(), chunks: self.chunks.clone() })
    }
}

impl ParallelVisitor for PtVisitor {
    fn visit(&mut self, entry: Result<ignore::DirEntry, ignore::Error>) -> WalkState {
        let Ok(entry) = entry else { return WalkState::Continue };
        if !entry.file_type().map_or(false, |ft| ft.is_file()) {
            return WalkState::Continue;
        }
        let p = entry.into_path();
        if want(&p) { self.local.push(p); }
        WalkState::Continue
    }
}

impl Drop for PtVisitor {
    fn drop(&mut self) {
        let v = std::mem::take(&mut self.local);
        if !v.is_empty() {
            self.chunks.lock().unwrap().push(v);
        }
    }
}

fn walk_per_thread(root: &str) -> Vec<PathBuf> {
    let chunks: Arc<Mutex<Vec<Vec<PathBuf>>>> = Arc::new(Mutex::new(Vec::new()));
    let mut builder = PtBuilder { chunks: chunks.clone() };
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build_parallel()
        .visit(&mut builder);
    let mut out = Vec::new();
    for chunk in std::mem::take(&mut *chunks.lock().unwrap()) {
        out.extend(chunk);
    }
    out
}

fn main() {
    let root = std::env::args().nth(1).expect("pass root");
    // Warmup
    let a = walk_mutex(&root);
    let b = walk_per_thread(&root);
    eprintln!("warmup: mutex={} per_thread={}", a.len(), b.len());

    for trial in 0..5 {
        let t = Instant::now();
        let m = walk_mutex(&root);
        let dm = t.elapsed();

        let t = Instant::now();
        let p = walk_per_thread(&root);
        let dp = t.elapsed();

        println!(
            "trial={} mutex={}files {}us   per_thread={}files {}us   speedup={:.2}x",
            trial, m.len(), dm.as_micros(), p.len(), dp.as_micros(),
            dm.as_secs_f64() / dp.as_secs_f64()
        );
    }
}
