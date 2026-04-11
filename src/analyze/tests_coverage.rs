use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kiss::{GateConfig, ParsedFile};

use crate::analyze::compute_test_coverage_from_lists;
use crate::analyze::CheckCoverageGateParams;
use tempfile::TempDir;

#[test]
fn test_gate_helpers_and_empty_analysis() {
    let gate = GateConfig {
        test_coverage_threshold: 0,
        ..Default::default()
    };
    let focus = HashSet::new();
    let p = CheckCoverageGateParams {
        py_parsed: &[],
        rs_parsed: &[],
        gate_config: &gate,
        focus_set: &focus,
        show_timing: false,
    };
    assert!(crate::analyze::check_coverage_gate(&p));
    let (cov, tested, total, unref) = compute_test_coverage_from_lists(&[], &[], &focus);
    assert_eq!(cov, 100);
    assert_eq!(tested, 0);
    assert_eq!(total, 0);
    assert!(unref.is_empty());
}

const WELL_PY: &str = r"def f1(): pass
def f2(): pass
def f3(): pass
def f4(): pass
def f5(): pass
def f6(): pass
def f7(): pass
def f8(): pass
def f9(): pass
";

const TEST_WELL_PY: &str = r"from well_covered import f1, f2, f3, f4, f5, f6, f7, f8, f9
def test_all():
    f1(); f2(); f3(); f4(); f5(); f6(); f7(); f8(); f9()
";

fn write_gate_py_sources(dir: &Path) {
    std::fs::write(dir.join("well_covered.py"), WELL_PY).unwrap();
    std::fs::write(dir.join("poorly_covered.py"), "def orphan_func():\n    pass\n").unwrap();
    std::fs::write(dir.join("test_well.py"), TEST_WELL_PY).unwrap();
}

fn parse_gate_py(dir: &Path) -> (Vec<ParsedFile>, HashSet<PathBuf>) {
    let py_files = vec![
        dir.join("well_covered.py"),
        dir.join("poorly_covered.py"),
        dir.join("test_well.py"),
    ];
    let results = kiss::parse_files(&py_files).unwrap();
    let py_parsed: Vec<ParsedFile> = results.into_iter().filter_map(Result::ok).collect();
    assert_eq!(py_parsed.len(), 3, "all 3 files should parse");
    let focus: HashSet<PathBuf> = py_parsed.iter().map(|p| p.path.clone()).collect();
    (py_parsed, focus)
}

fn write_per_file_gate_fixture(tmp: &TempDir) -> (Vec<ParsedFile>, HashSet<PathBuf>) {
    write_gate_py_sources(tmp.path());
    parse_gate_py(tmp.path())
}

/// Regression: per-file enforcement must fail when one file is below threshold
/// even if overall coverage would pass. With overall enforcement this would incorrectly pass.
#[test]
fn test_coverage_gate_per_file_fails_when_one_file_below_threshold() {
    let tmp = TempDir::new().unwrap();
    let (py_parsed, focus) = write_per_file_gate_fixture(&tmp);
    let gate = GateConfig {
        test_coverage_threshold: 90,
        ..Default::default()
    };
    let p = CheckCoverageGateParams {
        py_parsed: &py_parsed,
        rs_parsed: &[],
        gate_config: &gate,
        focus_set: &focus,
        show_timing: false,
    };
    assert!(
        !crate::analyze::check_coverage_gate(&p),
        "per-file enforcement must fail when one file (poorly_covered) is below 90%"
    );
}
