use std::path::Path;

use kiss::check_universe_cache::FullCheckCache;
use tempfile::TempDir;

use super::{AnalyzeOptions, run_analyze_with_result};
use crate::analyze_cache::test_helpers::ScopedHome;

fn cache_files() -> Vec<std::path::PathBuf> {
    let Ok(rd) = std::fs::read_dir(kiss::check_cache::cache_dir()) else {
        return Vec::new();
    };
    let mut files: Vec<_> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|name| name.starts_with("check_full_") && name.ends_with(".bin"))
        })
        .collect();
    files.sort();
    files
}

fn load_full_cache_file(path: &Path) -> Option<FullCheckCache> {
    let bytes = std::fs::read(path).ok()?;
    bincode::deserialize(&bytes).ok()
}

fn cache_mentions_universe(cache: &FullCheckCache, universe: &str) -> bool {
    cache
        .py_paths
        .iter()
        .chain(&cache.rs_paths)
        .any(|cached| cached == universe || cached.starts_with(&format!("{universe}/")))
}

fn cache_files_for_universe(universe: &str) -> Vec<std::path::PathBuf> {
    cache_files()
        .into_iter()
        .filter(|path| {
            load_full_cache_file(path)
                .is_some_and(|cache| cache_mentions_universe(&cache, universe))
        })
        .collect()
}

fn write_covered_rust_corpus(root: &Path) {
    write_cargo_manifest(root);
    std::fs::write(
        root.join("lib.rs"),
        "pub fn covered() -> i32 { 1 }\n\
         #[cfg(test)]\n\
         mod tests {\n\
             use super::covered;\n\
             #[test]\n\
             fn covers() { assert_eq!(covered(), 1); }\n\
         }\n",
    )
    .unwrap();
}

fn write_uncovered_rust_corpus(root: &Path) {
    write_cargo_manifest(root);
    std::fs::write(root.join("lib.rs"), "pub fn uncovered() -> i32 { 1 }\n").unwrap();
}

fn write_cargo_manifest(root: &Path) {
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"kiss-cache-contract-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\npath = \"lib.rs\"\n",
    )
    .unwrap();
}

fn analyze_options<'a>(
    universe: &'a str,
    focus_paths: &'a [String],
    py_config: &'a kiss::Config,
    rs_config: &'a kiss::Config,
    gate_config: &'a kiss::GateConfig,
) -> AnalyzeOptions<'a> {
    AnalyzeOptions {
        universe,
        focus_paths,
        py_config,
        rs_config,
        lang_filter: Some(kiss::Language::Rust),
        bypass_gate: false,
        gate_config,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    }
}

#[test]
fn default_check_writes_and_replays_full_cache() {
    let _home = ScopedHome::new();
    let repo = TempDir::new().unwrap();
    write_covered_rust_corpus(repo.path());

    let universe = repo.path().to_string_lossy().to_string();
    let focus = vec![universe.clone()];
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let opts = analyze_options(&universe, &focus, &py, &rs, &gate);

    let cold = run_analyze_with_result(&opts);
    assert!(cold.success, "covered corpus should pass default check");
    assert!(
        cold.metrics.is_some(),
        "cold run should compute metrics before cache exists"
    );
    let after_cold = cache_files_for_universe(&universe);
    assert_eq!(after_cold.len(), 1, "cold run should write one cache file");

    let warm = run_analyze_with_result(&opts);
    assert_eq!(warm.success, cold.success);
    assert!(
        warm.metrics.is_none(),
        "warm run should replay from full-check cache"
    );
    assert_eq!(cache_files_for_universe(&universe), after_cold);
}

#[test]
fn coverage_gate_failure_still_writes_full_cache() {
    if std::env::var_os("CARGO_LLVM_COV").is_some() {
        // The product code suppresses nested cargo-llvm-cov collection, so this
        // contract is exercised by normal cargo test rather than coverage runs.
        return;
    }
    let _home = ScopedHome::new();
    let repo = TempDir::new().unwrap();
    write_uncovered_rust_corpus(repo.path());

    let universe = repo.path().to_string_lossy().to_string();
    let focus = vec![universe.clone()];
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let opts = analyze_options(&universe, &focus, &py, &rs, &gate);

    let result = run_analyze_with_result(&opts);
    assert!(
        !result.success,
        "uncovered production function should fail the coverage gate"
    );
    assert_eq!(
        cache_files_for_universe(&universe).len(),
        1,
        "coverage-gate failure should still write a replayable cache"
    );
}
