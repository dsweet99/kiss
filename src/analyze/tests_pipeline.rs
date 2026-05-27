use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use kiss::{Config, GateConfig};
use tempfile::TempDir;

#[test]
fn run_full_pipeline_parses_small_python_file() {
    let tmp = TempDir::new().unwrap();
    let py_path = tmp.path().join("mod.py");
    std::fs::write(&py_path, "def hello():\n    pass\n").unwrap();
    let py_files = vec![py_path.clone()];
    let rs_files: Vec<PathBuf> = Vec::new();
    let focus: HashSet<PathBuf> = std::iter::once(py_path).collect();
    let gate = GateConfig::default();
    let py_cfg = Config::python_defaults();
    let rs_cfg = Config::rust_defaults();
    let paths = vec![tmp.path().to_string_lossy().to_string()];
    let opts = crate::analyze::AnalyzeOptions {
        universe: tmp.path().to_str().unwrap(),
        focus_paths: &paths,
        py_config: &py_cfg,
        rs_config: &rs_cfg,
        lang_filter: None,
        bypass_gate: true,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
    };
    let now = Instant::now();
    let pipeline = crate::analyze::run_full_pipeline(crate::analyze::FullPipelineInput {
        opts: &opts,
        py_files: &py_files,
        rs_files: &rs_files,
        focus_set: &focus,
        t0: now,
        t1: now,
        t2: now,
    });
    assert_eq!(pipeline.file_count, 1);
    assert_eq!(pipeline.result.py_parsed.len(), 1);
    assert!(!pipeline.py_stats.statements_per_function.is_empty());
}
