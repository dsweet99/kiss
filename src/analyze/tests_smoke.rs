use std::collections::HashSet;
use std::path::Path;

use kiss::{Config, GateConfig};

use crate::analyze::graph_api::graph_stats;
use crate::analyze::options::AnalyzeOptions;
use crate::analyze::print::{PrintResultsCtx, print_analysis_summary};
use crate::analyze::{
    AnalyzeGraphsIn, GraphConfigs, filter_duplicates_by_focus, gather_files, run_analyze,
};
use crate::analyze_parse::ParseResult;
use tempfile::TempDir;

fn tmp_repo_three_files() -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.py"), "import b\ndef f(): pass").unwrap();
    std::fs::write(tmp.path().join("b.py"), "x=1").unwrap();
    std::fs::write(tmp.path().join("c.rs"), "fn main() {}").unwrap();
    tmp
}

#[test]
fn test_structs() {
    let py_cfg = Config::python_defaults();
    let rs_cfg = Config::rust_defaults();
    let gate_cfg = GateConfig::default();
    let _ = AnalyzeOptions {
        universe: ".",
        focus_paths: &[],
        py_config: &py_cfg,
        rs_config: &rs_cfg,
        lang_filter: None,
        bypass_gate: false,
        gate_config: &gate_cfg,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
    };
    let _ = ParseResult {
        py_parsed: vec![],
        rs_parsed: vec![],
        violations: vec![],
        code_unit_count: 0,
        statement_count: 0,
    };
}

#[test]
fn test_gather_parse_and_graphs() {
    let tmp = tmp_repo_three_files();
    let (py, rs) = gather_files(tmp.path(), None, &[]);
    assert_eq!(py.len(), 2);
    assert_eq!(rs.len(), 1);
    assert!(
        !crate::analyze::focus::build_focus_set(
            &[tmp.path().to_string_lossy().to_string()],
            None,
            &[]
        )
        .is_empty()
    );

    let result = crate::analyze_parse::parse_all(
        &py,
        &rs,
        &Config::python_defaults(),
        &Config::rust_defaults(),
    );
    assert_eq!(result.py_parsed.len(), 2);
    assert_eq!(result.rs_parsed.len(), 1);

    let (py_g, rs_g) = crate::analyze::build_graphs(&result.py_parsed, &result.rs_parsed);
    assert!(py_g.is_some());
    let gate = GateConfig::default();
    let cfg = GraphConfigs {
        py_config: &Config::python_defaults(),
        rs_config: &Config::rust_defaults(),
        gate: &gate,
    };
    let _ = crate::analyze::analyze_graphs(&AnalyzeGraphsIn {
        py_graph: py_g.as_ref(),
        rs_graph: rs_g.as_ref(),
        configs: cfg,
    });
}

#[test]
fn test_print_functions_and_helpers() {
    print_analysis_summary(&kiss::GlobalMetrics::default(), None, None);
    let (n, e) = graph_stats(None, None);
    assert_eq!(n, 0);
    assert_eq!(e, 0);
    assert!(crate::analyze::is_focus_file(
        Path::new("any.py"),
        &HashSet::new()
    ));
    let dups = filter_duplicates_by_focus(vec![], &HashSet::new());
    assert!(dups.is_empty());
}

#[test]
fn test_detect_duplicates() {
    let py_dups = crate::analyze::detect_py_duplicates(&[], 0.7);
    assert!(py_dups.is_empty());
    let rs_dups = crate::analyze::detect_rs_duplicates(&[], 0.7);
    assert!(rs_dups.is_empty());
}

#[test]
fn test_print_all_results() {
    let result = crate::analyze::print::print_all_results_with_dups(
        &[],
        &[],
        &[],
        PrintResultsCtx {
            show_timing: false,
            t_phase2: None,
            suppress_final_status: false,
        },
    );
    assert!(result);
}

#[test]
fn test_run_analyze_no_files() {
    let tmp = TempDir::new().unwrap();
    let py_cfg = Config::python_defaults();
    let rs_cfg = Config::rust_defaults();
    let gate_cfg = GateConfig::default();
    let opts = AnalyzeOptions {
        universe: tmp.path().to_str().unwrap(),
        focus_paths: &[],
        py_config: &py_cfg,
        rs_config: &rs_cfg,
        lang_filter: None,
        bypass_gate: true,
        gate_config: &gate_cfg,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
    };
    assert!(run_analyze(&opts));

    std::fs::write(tmp.path().join("lib.rs"), "fn foo() { let x = 1; }").unwrap();
    let (parsed, viols, units, _) = crate::analyze_parse::parse_and_analyze_rs(
        &[tmp.path().join("lib.rs")],
        &Config::rust_defaults(),
    );
    assert_eq!(parsed.len(), 1);
    assert!(viols.is_empty());
    assert!(units > 0);
}
