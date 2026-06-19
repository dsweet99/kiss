use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::analyze::FocusFilter;
use kiss::{
    CodeDefinition, CodeUnitKind, GateConfig, ParsedFile, RustTestRefAnalysis, TestRefAnalysis,
};

use crate::analyze::CheckCoverageGateParams;
use crate::analyze::compute_test_coverage_from_lists;
use tempfile::TempDir;

fn empty_py_cov() -> TestRefAnalysis {
    TestRefAnalysis {
        definitions: Vec::new(),
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        unreferenced: Vec::new(),
        coverage_map: HashMap::new(),
    }
}

fn empty_rs_cov() -> RustTestRefAnalysis {
    RustTestRefAnalysis {
        definitions: Vec::new(),
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        propagated_references: HashSet::new(),
        unreferenced: Vec::new(),
        coverage_map: HashMap::new(),
    }
}

fn runtime_line_def(file: &Path, line: usize) -> CodeDefinition {
    CodeDefinition {
        name: format!("line_{line}"),
        kind: CodeUnitKind::Module,
        file: file.to_path_buf(),
        line,
        containing_class: None,
    }
}

#[test]
fn test_gate_helpers_and_empty_analysis() {
    let gate = GateConfig {
        test_coverage_threshold: 0,
        ..Default::default()
    };
    let focus = FocusFilter::unrestricted();
    let py_cov = empty_py_cov();
    let rs_cov = empty_rs_cov();
    let p = CheckCoverageGateParams {
        py_cov: &py_cov,
        rs_cov: &rs_cov,
        gate_config: &gate,
        focus: &focus,
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
    std::fs::write(
        dir.join("poorly_covered.py"),
        "def orphan_func():\n    pass\n",
    )
    .unwrap();
    std::fs::write(dir.join("test_well.py"), TEST_WELL_PY).unwrap();
}

fn parse_gate_py(dir: &Path) -> (Vec<ParsedFile>, FocusFilter) {
    let py_files = vec![
        dir.join("well_covered.py"),
        dir.join("poorly_covered.py"),
        dir.join("test_well.py"),
    ];
    let results = kiss::parse_files(&py_files).unwrap();
    let py_parsed: Vec<ParsedFile> = results.into_iter().filter_map(Result::ok).collect();
    assert_eq!(py_parsed.len(), 3, "all 3 files should parse");
    let paths: HashSet<PathBuf> = py_parsed.iter().map(|p| p.path.clone()).collect();
    (py_parsed, FocusFilter::restricting(paths))
}

fn write_per_file_gate_fixture(tmp: &TempDir) -> (Vec<ParsedFile>, FocusFilter) {
    write_gate_py_sources(tmp.path());
    parse_gate_py(tmp.path())
}

#[test]
fn test_write_gate_py_sources_creates_files() {
    let tmp = TempDir::new().unwrap();
    write_gate_py_sources(tmp.path());
    assert!(tmp.path().join("well_covered.py").exists());
    assert!(tmp.path().join("poorly_covered.py").exists());
    assert!(tmp.path().join("test_well.py").exists());
}

#[test]
fn test_parse_gate_py_returns_three_files() {
    let tmp = TempDir::new().unwrap();
    write_gate_py_sources(tmp.path());
    let (parsed, focus) = parse_gate_py(tmp.path());
    assert_eq!(parsed.len(), 3);
    assert_eq!(focus.paths().len(), 3);
}

/// Regression: per-file enforcement must fail when one file is below threshold
/// even if overall coverage would pass. With overall enforcement this would incorrectly pass.
#[test]
fn test_coverage_gate_per_file_fails_when_one_file_below_threshold() {
    let tmp = TempDir::new().unwrap();
    let (py_parsed, focus) = write_per_file_gate_fixture(&tmp);
    let well = py_parsed
        .iter()
        .find(|file| file.path.ends_with("well_covered.py"))
        .expect("well-covered fixture should parse");
    let poor = py_parsed
        .iter()
        .find(|file| file.path.ends_with("poorly_covered.py"))
        .expect("poorly-covered fixture should parse");
    let mut py_cov = empty_py_cov();
    py_cov
        .definitions
        .extend((1..=9).map(|line| runtime_line_def(&well.path, line)));
    let missed = runtime_line_def(&poor.path, 1);
    py_cov.definitions.push(missed.clone());
    py_cov.unreferenced.push(missed);
    let rs_cov = empty_rs_cov();
    let gate = GateConfig {
        test_coverage_threshold: 90,
        ..Default::default()
    };
    let p = CheckCoverageGateParams {
        py_cov: &py_cov,
        rs_cov: &rs_cov,
        gate_config: &gate,
        focus: &focus,
        show_timing: false,
    };
    assert!(
        !crate::analyze::check_coverage_gate(&p),
        "per-file enforcement must fail when one file (poorly_covered) is below 90%"
    );
}

#[test]
fn coverage_struct_sizes_for_gate() {
    use crate::analyze::coverage::{CoverageOutputOpts, GraphRefPair};
    use crate::analyze::coverage_types::{CheckCoverageGateParams, PyRsTestCoverage};
    let _ = std::mem::size_of::<GraphRefPair>();
    let _ = std::mem::size_of::<CoverageOutputOpts>();
    let _ = std::mem::size_of::<PyRsTestCoverage>();
    let _ = std::mem::size_of::<CheckCoverageGateParams>();
}

#[test]
fn coverage_build_viols_after_merge_empty() {
    use crate::analyze::coverage::{GraphRefPair, build_viols_after_merge};
    let definitions = vec![];
    let unreferenced = vec![];
    let focus = crate::analyze::FocusFilter::unrestricted();
    let graphs = GraphRefPair { py: None, rs: None };
    let (viols, defs, unref) =
        build_viols_after_merge(definitions, unreferenced, &focus, graphs, None, false);
    assert!(viols.is_empty());
    assert!(defs.is_empty());
    assert!(unref.is_empty());
}

#[test]
fn coverage_build_viols_after_merge_with_unreferenced() {
    use crate::analyze::coverage::{GraphRefPair, build_viols_after_merge};
    use kiss::check_universe_cache::CachedCoverageItem;
    let definitions = vec![CachedCoverageItem {
        file: "/tmp/test.py".to_string(),
        name: "foo".to_string(),
        line: 1,
    }];
    let unreferenced = vec![CachedCoverageItem {
        file: "/tmp/test.py".to_string(),
        name: "foo".to_string(),
        line: 1,
    }];
    let focus = crate::analyze::FocusFilter::restricting(
        std::iter::once(PathBuf::from("/tmp/test.py")).collect(),
    );
    let graphs = GraphRefPair { py: None, rs: None };
    let (viols, _, _) =
        build_viols_after_merge(definitions, unreferenced, &focus, graphs, None, false);
    assert_eq!(viols.len(), 1);
    assert!(viols[0].message.contains("0% covered"));
}

#[test]
fn coverage_weighted_sentinel_respects_focus_set() {
    use crate::analyze::coverage::{GraphRefPair, build_viols_after_merge};
    use kiss::check_universe_cache::CachedCoverageItem;
    use std::collections::HashMap;
    let out_of_focus = PathBuf::from("/tmp/out.py");
    let in_focus = PathBuf::from("/tmp/in.py");
    let definitions = vec![
        CachedCoverageItem {
            file: out_of_focus.to_string_lossy().to_string(),
            name: "big".into(),
            line: 1,
        },
        CachedCoverageItem {
            file: in_focus.to_string_lossy().to_string(),
            name: "g".into(),
            line: 1,
        },
    ];
    let focus =
        crate::analyze::FocusFilter::restricting(std::iter::once(in_focus.clone()).collect());
    let mut weighted = HashMap::new();
    weighted.insert(out_of_focus.clone(), 0);
    weighted.insert(in_focus.clone(), 0);
    let graphs = GraphRefPair { py: None, rs: None };
    let (viols, _, _) =
        build_viols_after_merge(definitions, vec![], &focus, graphs, Some(&weighted), false);
    assert_eq!(viols.len(), 1);
    assert_eq!(viols[0].file, in_focus);
}
