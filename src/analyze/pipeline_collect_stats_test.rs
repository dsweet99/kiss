use super::*;
use crate::analyze::FocusFilter;
use crate::analyze::pipeline_gate_failure::{
    CoverageGateFailureCacheIn, should_store_coverage_gate_failure_cache,
    store_coverage_gate_failure_cache,
};
use crate::analyze_parse::ParseResult;
use std::path::PathBuf;

fn options<'a>(
    collect_stats: bool,
    py_config: &'a kiss::Config,
    rs_config: &'a kiss::Config,
    gate_config: &'a kiss::GateConfig,
) -> AnalyzeOptions<'a> {
    AnalyzeOptions {
        universe: ".",
        focus_paths: &[],
        py_config,
        rs_config,
        lang_filter: None,
        bypass_gate: true,
        gate_config,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: true,
        jobs: Some(1),
        collect_stats,
    }
}

fn empty_parse_result() -> ParseResult {
    ParseResult {
        py_parsed: Vec::new(),
        rs_parsed: Vec::new(),
        violations: Vec::new(),
        code_unit_count: 0,
        statement_count: 0,
    }
}

fn timing_options<'a>(
    py_config: &'a kiss::Config,
    rs_config: &'a kiss::Config,
    gate_config: &'a kiss::GateConfig,
) -> AnalyzeOptions<'a> {
    AnalyzeOptions {
        show_timing: true,
        ..options(false, py_config, rs_config, gate_config)
    }
}

fn parsed_rs(path: PathBuf) -> kiss::ParsedRustFile {
    let source = "pub fn uncovered() -> usize { 1 }\n".to_string();
    let ast = syn::parse_file(&source).unwrap();
    kiss::ParsedRustFile { path, source, ast }
}

#[test]
fn full_pipeline_collects_metric_stats_only_when_requested() {
    let py_config = kiss::Config::python_defaults();
    let rs_config = kiss::Config::rust_defaults();
    let gate_config = kiss::GateConfig::default();
    let focus = FocusFilter::unrestricted();

    let without_stats = run_full_pipeline_with_parse(FullPipelineWithParseInput {
        opts: &options(false, &py_config, &rs_config, &gate_config),
        focus: &focus,
        result: empty_parse_result(),
        parse_timing: String::new(),
        timings: (
            std::time::Instant::now(),
            std::time::Instant::now(),
            std::time::Instant::now(),
        ),
    });
    assert!(without_stats.py_stats.is_none());
    assert!(without_stats.rs_stats.is_none());

    let with_stats = run_full_pipeline_with_parse(FullPipelineWithParseInput {
        opts: &options(true, &py_config, &rs_config, &gate_config),
        focus: &focus,
        result: empty_parse_result(),
        parse_timing: String::new(),
        timings: (
            std::time::Instant::now(),
            std::time::Instant::now(),
            std::time::Instant::now(),
        ),
    });
    assert!(with_stats.py_stats.is_some());
    assert!(with_stats.rs_stats.is_some());
}

#[test]
fn check_all_uses_fail_closed_rust_runtime_coverage() {
    let py_config = kiss::Config::python_defaults();
    let rs_config = kiss::Config::rust_defaults();
    let gate_config = kiss::GateConfig::default();
    let opts = options(false, &py_config, &rs_config, &gate_config);
    let parsed = vec![parsed_rs(PathBuf::from("src/lib.rs"))];

    let analysis = runtime_rust_coverage_for_opts(&opts, &parsed);

    assert_eq!(analysis.definitions.len(), 1);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.definitions[0].name, "llvm_cov_failed");
    assert_eq!(analysis.unreferenced[0].file, PathBuf::from("src/lib.rs"));
}

#[test]
fn rust_coverage_policy_is_fail_closed_only_for_check_all() {
    let py_config = kiss::Config::python_defaults();
    let rs_config = kiss::Config::rust_defaults();
    let gate_config = kiss::GateConfig::default();
    let check_all = options(false, &py_config, &rs_config, &gate_config);
    let stats = options(true, &py_config, &rs_config, &gate_config);
    let mut gated = options(false, &py_config, &rs_config, &gate_config);
    gated.bypass_gate = false;

    assert!(use_fail_closed_rust_coverage(&check_all));
    assert!(!use_fail_closed_rust_coverage(&stats));
    assert!(!use_fail_closed_rust_coverage(&gated));
}

#[test]
fn empty_rust_runtime_coverage_has_no_units() {
    let analysis = empty_rust_runtime_coverage();

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn full_pipeline_wrapper_handles_empty_inputs() {
    let py_config = kiss::Config::python_defaults();
    let rs_config = kiss::Config::rust_defaults();
    let gate_config = kiss::GateConfig::default();
    let opts = options(false, &py_config, &rs_config, &gate_config);
    let focus = FocusFilter::unrestricted();
    let now = std::time::Instant::now();

    let result = run_full_pipeline(FullPipelineInput {
        opts: &opts,
        py_files: &[],
        rs_files: &[],
        focus: &focus,
        t0: now,
        t1: now,
        t2: now,
    });

    assert_eq!(result.file_count, 0);
    assert!(result.py_stats.is_none());
    assert!(result.rs_stats.is_none());
}

#[test]
fn coverage_gate_failure_cache_size_limit_is_explicit() {
    let small_py = kiss::TestRefAnalysis {
        definitions: Vec::new(),
        test_references: Default::default(),
        call_references: Default::default(),
        unreferenced: Vec::new(),
        coverage_map: Default::default(),
    };
    let mut large_rs = empty_rust_runtime_coverage();
    large_rs.unreferenced = (0..10_001)
        .map(|line| kiss::RustCodeDefinition {
            name: format!("line_{line}"),
            kind: kiss::CodeUnitKind::Module,
            file: PathBuf::from("src/lib.rs"),
            line,
            impl_for_type: None,
        })
        .collect();

    assert!(should_store_coverage_gate_failure_cache(
        &small_py,
        &empty_rust_runtime_coverage()
    ));
    assert!(!should_store_coverage_gate_failure_cache(
        &small_py, &large_rs
    ));
}

#[test]
fn uncached_analysis_finalizes_without_metric_stats_for_check_mode() {
    let py_config = kiss::Config::python_defaults();
    let rs_config = kiss::Config::rust_defaults();
    let gate_config = kiss::GateConfig::default();
    let focus = FocusFilter::unrestricted();
    let opts = options(false, &py_config, &rs_config, &gate_config);
    let now = std::time::Instant::now();

    let result = run_analyze_uncached(crate::analyze::params::RunAnalyzeUncached {
        opts: &opts,
        py_files: &[],
        rs_files: &[],
        focus: &focus,
        t0: now,
        t1: now,
    });

    assert!(result.success);
    assert_eq!(result.metrics.unwrap().files, 0);
}

#[test]
fn uncached_analysis_finalizes_with_metric_stats_for_stats_mode() {
    let py_config = kiss::Config::python_defaults();
    let rs_config = kiss::Config::rust_defaults();
    let gate_config = kiss::GateConfig::default();
    let focus = FocusFilter::unrestricted();
    let opts = options(true, &py_config, &rs_config, &gate_config);
    let now = std::time::Instant::now();

    let result = run_analyze_uncached(crate::analyze::params::RunAnalyzeUncached {
        opts: &opts,
        py_files: &[],
        rs_files: &[],
        focus: &focus,
        t0: now,
        t1: now,
    });

    assert!(result.success);
    assert_eq!(result.metrics.unwrap().code_units, 0);
}

#[test]
fn coverage_gate_failure_cache_accepts_precomputed_lists() {
    let py_config = kiss::Config::python_defaults();
    let rs_config = kiss::Config::rust_defaults();
    let gate_config = kiss::GateConfig::default();
    let focus = FocusFilter::unrestricted();
    let opts = timing_options(&py_config, &rs_config, &gate_config);
    let result = empty_parse_result();

    store_coverage_gate_failure_cache(CoverageGateFailureCacheIn {
        opts: &opts,
        py_files: &[],
        rs_files: &[],
        focus: &focus,
        result: &result,
        py_cov: kiss::TestRefAnalysis {
            definitions: Vec::new(),
            test_references: Default::default(),
            call_references: Default::default(),
            unreferenced: Vec::new(),
            coverage_map: Default::default(),
        },
        rs_cov: kiss::RustTestRefAnalysis {
            definitions: Vec::new(),
            test_references: Default::default(),
            call_references: Default::default(),
            propagated_references: Default::default(),
            unreferenced: Vec::new(),
            coverage_map: Default::default(),
        },
        rs_graph: None,
    });
}

#[test]
fn check_all_uncached_analysis_fails_with_fail_closed_rust_coverage() {
    let tmp = tempfile::TempDir::new().unwrap();
    let rs = tmp.path().join("lib.rs");
    std::fs::write(&rs, "pub fn uncovered() -> usize { 1 }\n").unwrap();
    let py_config = kiss::Config::python_defaults();
    let rs_config = kiss::Config::rust_defaults();
    let gate_config = kiss::GateConfig {
        test_coverage_threshold: 1,
        ..kiss::GateConfig::default()
    };
    let focus = FocusFilter::unrestricted();
    let mut opts = options(false, &py_config, &rs_config, &gate_config);
    opts.universe = tmp.path().to_str().unwrap();
    let now = std::time::Instant::now();

    let result = run_analyze_uncached(crate::analyze::params::RunAnalyzeUncached {
        opts: &opts,
        py_files: &[],
        rs_files: &[rs],
        focus: &focus,
        t0: now,
        t1: now,
    });

    assert!(
        !result.success,
        "fail-closed Rust coverage should make check --all report violations"
    );
}

#[test]
fn default_gate_stops_after_python_failure_before_rust_runtime() {
    let tmp = tempfile::TempDir::new().unwrap();
    let py = tmp.path().join("uncovered.py");
    let rs = tmp.path().join("lib.rs");
    std::fs::write(&py, "def uncovered():\n    return 1\n").unwrap();
    std::fs::write(&rs, "pub fn uncovered() -> usize { 1 }\n").unwrap();
    let py_config = kiss::Config::python_defaults();
    let rs_config = kiss::Config::rust_defaults();
    let gate_config = kiss::GateConfig {
        test_coverage_threshold: 1,
        ..kiss::GateConfig::default()
    };
    let focus = FocusFilter::unrestricted();
    let mut opts = options(false, &py_config, &rs_config, &gate_config);
    opts.bypass_gate = false;
    opts.universe = tmp.path().to_str().unwrap();
    let now = std::time::Instant::now();

    let result = run_analyze_uncached(crate::analyze::params::RunAnalyzeUncached {
        opts: &opts,
        py_files: &[py],
        rs_files: &[rs],
        focus: &focus,
        t0: now,
        t1: now,
    });

    assert!(
        !result.success,
        "Python coverage failure should decide the default gate before Rust runtime coverage"
    );
}
