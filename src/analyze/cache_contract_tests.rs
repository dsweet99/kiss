use std::path::Path;

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

fn write_covered_rust_corpus(root: &Path) {
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
    std::fs::write(root.join("lib.rs"), "pub fn uncovered() -> i32 { 1 }\n").unwrap();
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
    let after_cold = cache_files();
    assert_eq!(after_cold.len(), 1, "cold run should write one cache file");

    let warm = run_analyze_with_result(&opts);
    assert_eq!(warm.success, cold.success);
    assert!(
        warm.metrics.is_none(),
        "warm run should replay from full-check cache"
    );
    assert_eq!(cache_files(), after_cold);
}

#[test]
fn coverage_gate_failure_still_writes_full_cache() {
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
        cache_files().len(),
        1,
        "coverage-gate failure should still write a replayable cache"
    );
}
