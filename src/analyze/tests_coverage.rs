use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use kiss::{GateConfig, ParsedFile};

use crate::analyze::CheckCoverageGateParams;
use crate::analyze::compute_test_coverage_from_lists;
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
    std::fs::write(
        dir.join("poorly_covered.py"),
        "def orphan_func():\n    pass\n",
    )
    .unwrap();
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
    assert_eq!(focus.len(), 3);
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

#[test]
fn coverage_struct_sizes_for_gate() {
    use crate::analyze::coverage::{CoverageOutputOpts, GraphRefPair};
    use crate::analyze::coverage_types::{
        CheckCoverageGateParams, CoverageViolationSpec, PyRsTestCoverage,
    };
    let _ = std::mem::size_of::<GraphRefPair>();
    let _ = std::mem::size_of::<CoverageOutputOpts>();
    let _ = std::mem::size_of::<PyRsTestCoverage>();
    let _ = std::mem::size_of::<CoverageViolationSpec>();
    let _ = std::mem::size_of::<CheckCoverageGateParams>();
}

#[test]
fn coverage_build_viols_after_merge_empty() {
    use crate::analyze::coverage::{GraphRefPair, build_viols_after_merge};
    let definitions = vec![];
    let unreferenced = vec![];
    let focus_set: HashSet<PathBuf> = HashSet::new();
    let graphs = GraphRefPair { py: None, rs: None };
    let (viols, defs, unref) =
        build_viols_after_merge(definitions, unreferenced, &focus_set, graphs, None);
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
    let focus_set: HashSet<PathBuf> = std::iter::once(PathBuf::from("/tmp/test.py")).collect();
    let graphs = GraphRefPair { py: None, rs: None };
    let (viols, _, _) = build_viols_after_merge(definitions, unreferenced, &focus_set, graphs, None);
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
    let focus_set: HashSet<PathBuf> = std::iter::once(in_focus.clone()).collect();
    let mut weighted = HashMap::new();
    weighted.insert(out_of_focus.clone(), 0);
    weighted.insert(in_focus.clone(), 0);
    let graphs = GraphRefPair { py: None, rs: None };
    let (viols, _, _) = build_viols_after_merge(
        definitions,
        vec![],
        &focus_set,
        graphs,
        Some(&weighted),
    );
    assert_eq!(viols.len(), 1);
    assert_eq!(viols[0].file, in_focus);
}

#[test]
fn foil058_rs_orchestrate_referenced_via_run_rust_analysis() {
    use crate::analyze::gather_files;
    use crate::analyze::parallel::run_rust_analysis;
    use kiss::Language;
    use kiss::rust_test_refs::compute_rs_weighted_file_pcts;

    let root = Path::new("/tmp/kiss_foil_058jr492");
    if !root.exists() {
        return;
    }
    let (_py, rs_files) = gather_files(root, Some(Language::Rust), &[]);
    let parsed: Vec<_> = rs_files
        .iter()
        .filter_map(|p| kiss::parse_rust_file(p).ok())
        .collect();
    let gate = GateConfig::default();
    let analysis = run_rust_analysis(&parsed, &gate, None);
    let cliff = root.join("src/cliffs/cliff_00.rs");
    let orch_unref = analysis
        .cov
        .unreferenced
        .iter()
        .any(|d| d.file == cliff && d.name == "orchestrate");
    let refs: Vec<_> = parsed.iter().collect();
    let weighted = compute_rs_weighted_file_pcts(&analysis.cov, &refs);
    let pct = weighted.get(&cliff).copied().unwrap_or(100);
    assert!(
        !orch_unref,
        "pipeline rust analysis should keep orchestrate referenced"
    );
    assert!(
        (1..=5).contains(&pct),
        "expected weighted pct ~2% from pipeline path, got {pct}%"
    );
}

#[test]
fn gfqe6dy_collect_coverage_viols_reports_phantom_weighted_pct() {
    use crate::analyze::coverage::{CoverageOutputOpts, GraphRefPair, collect_coverage_viols};
    use crate::analyze::coverage_types::PyRsTestCoverage;
    use crate::analyze::gather_files;
    use crate::analyze::parallel::run_rust_analysis;

    let root = Path::new("/tmp/kiss_foil_gfqe6dy_");
    if !root.exists() {
        return;
    }
    let (_py, rs_files) = gather_files(root, None, &[]);
    let rs_parsed: Vec<_> = rs_files
        .iter()
        .filter_map(|p| kiss::parse_rust_file(p).ok())
        .collect();
    let gate = GateConfig::default();
    let rs = run_rust_analysis(&rs_parsed, &gate, None);
    let mut focus_set = HashSet::new();
    focus_set.extend(rs_files.iter().cloned());
    let graphs = GraphRefPair {
        py: None,
        rs: rs.graph.as_ref(),
    };
    let (viols, _) = collect_coverage_viols(
        PyRsTestCoverage {
            py: kiss::TestRefAnalysis {
                definitions: Vec::new(),
                test_references: HashSet::new(),
                call_references: HashSet::new(),
                unreferenced: Vec::new(),
                coverage_map: HashMap::new(),
            },
            rs: rs.cov,
        },
        &[],
        &rs_parsed,
        &focus_set,
        CoverageOutputOpts {
            bypass_gate: true,
            show_timing: false,
        },
        graphs,
        rs_files.as_slice(),
    );
    let phantom = root.join("src/phantom.rs");
    let phantom_viol = viols.iter().find(|v| v.file == phantom);
    assert!(
        phantom_viol.is_some(),
        "expected phantom.rs test_coverage violation, got {} viols: {:?}",
        viols.len(),
        viols.iter().map(|v| (&v.file, &v.unit_name, &v.message)).collect::<Vec<_>>()
    );
    let msg = &phantom_viol.unwrap().message;
    assert!(
        msg.contains("0% covered") || msg.contains("1% covered"),
        "phantom violation should carry low weighted pct, got: {msg}"
    );
}

#[test]
fn foil058_collect_coverage_viols_uses_weighted_file_pct() {
    use crate::analyze::coverage::{CoverageOutputOpts, GraphRefPair, collect_coverage_viols};
    use crate::analyze::coverage_types::PyRsTestCoverage;
    use crate::analyze::gather_files;
    use crate::analyze::parallel::run_rust_analysis;

    let root = Path::new("/tmp/kiss_foil_058jr492");
    if !root.exists() {
        return;
    }
    let (py_files, rs_files) = gather_files(root, None, &[]);
    let py_parsed: Vec<_> = py_files
        .iter()
        .filter_map(|p| kiss::parse_files(std::slice::from_ref(p)).ok())
        .flat_map(|v| v.into_iter().flatten())
        .collect();
    let rs_parsed: Vec<_> = rs_files
        .iter()
        .filter_map(|p| kiss::parse_rust_file(p).ok())
        .collect();
    let gate = GateConfig::default();
    let rs = run_rust_analysis(&rs_parsed, &gate, None);
    let py_refs: Vec<_> = py_parsed.iter().collect();
    let py_cov = kiss::analyze_test_refs(&py_refs, None);
    let mut focus_set = HashSet::new();
    focus_set.extend(py_files.iter().cloned());
    focus_set.extend(rs_files.iter().cloned());
    let graphs = GraphRefPair {
        py: None,
        rs: rs.graph.as_ref(),
    };
    let (viols, _) = collect_coverage_viols(
        PyRsTestCoverage {
            py: py_cov,
            rs: rs.cov,
        },
        &py_parsed,
        &rs_parsed,
        &focus_set,
        CoverageOutputOpts {
            bypass_gate: true,
            show_timing: false,
        },
        graphs,
        rs_files.as_slice(),
    );
    let cliff = root.join("src/cliffs/cliff_00.rs");
    let handler_viol = viols
        .iter()
        .find(|v| v.file == cliff && v.unit_name == "handler_0");
    let Some(v) = handler_viol else {
        panic!(
            "expected handler_0 violation on cliff_00, got {} viols",
            viols.len()
        );
    };
    assert!(
        v.message.contains("2% covered") || v.message.contains("1% covered"),
        "handler violation should carry weighted file pct, got: {}",
        v.message
    );
}

#[test]
fn coverage_inject_binary_entry_sentinels_adds_unreferenced_entry_for_bin_files() {
    use crate::analyze::coverage_weighted::inject_binary_entry_sentinels;
    use kiss::check_universe_cache::CachedCoverageItem;
    let mut definitions: Vec<CachedCoverageItem> = vec![];
    let mut unreferenced: Vec<CachedCoverageItem> = vec![];
    let bin = PathBuf::from("/tmp/proj/src/bin/runner.rs");
    inject_binary_entry_sentinels(
        &mut definitions,
        &mut unreferenced,
        std::slice::from_ref(&bin),
    );
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].name, "__entry_point__");
    assert_eq!(unreferenced.len(), 1);
}
