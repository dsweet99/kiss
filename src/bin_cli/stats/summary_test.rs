use super::*;
use crate::test_runner::capture_stdout::capture_stdout;
use kiss::{Config, GateConfig};
use std::path::PathBuf;

fn nested_runtime_gate() -> GateConfig {
    GateConfig {
        max_unit_test_seconds: vec![
            ("tests/slow/dbs".into(), 180.0),
            ("tests/slow".into(), 60.0),
            ("*".into(), 0.0),
        ],
        ..GateConfig::default()
    }
}

fn assert_nested_runtime_table(stdout: &str) {
    assert!(
        stdout.contains("unit_test_runtime_sec:"),
        "missing runtime heading:\n{stdout}"
    );
    let rows: Vec<&str> = stdout
        .lines()
        .skip_while(|l| !l.starts_with("unit_test_runtime_sec:"))
        .skip(2)
        .take_while(|l| l.contains('\t'))
        .collect();
    assert_eq!(rows.len(), 3, "rows:\n{}\nfull:\n{stdout}", rows.join("\n"));
    assert!(rows[0].starts_with("tests/slow/dbs\t180\t0\t"), "{}", rows[0]);
    assert!(rows[1].starts_with("tests/slow\t60\t0\t"), "{}", rows[1]);
    assert!(rows[2].starts_with("*\t0\t0\t"), "{}", rows[2]);
}

#[test]
fn maybe_print_cached_stats_summary_returns_false_on_miss() {
    let paths = vec![".".to_string()];
    let py: Vec<PathBuf> = Vec::new();
    let rs: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    assert!(!maybe_print_cached_stats_summary(CachedStatsSummaryArgs {
        paths: &paths,
        py_files: &py,
        rs_files: &rs,
        py_cfg: &py_cfg,
        rs_cfg: &rs_cfg,
        gate: &gate,
        lang_filter: None,
        ignore: &[],
    }));
}

#[test]
fn print_cached_summary_forwards_resolved_gate_runtime_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = [tmp.path().to_string_lossy().into_owned()];
    let cache = FullCheckCache {
        fingerprint: "test".into(),
        py_stats: None,
        rs_stats: None,
        py_paths: vec![],
        focus_paths: vec![],
        focus_restrict: false,
        rs_paths: vec![],
        py_file_count: 1,
        rs_file_count: 0,
        code_unit_count: 3,
        statement_count: 5,
        graph_nodes: 1,
        graph_edges: 0,
        base_violations: vec![],
        graph_violations: vec![],
        py_duplicates: vec![],
        rs_duplicates: vec![],
        file_content_digests: vec![],
    };
    let gate = nested_runtime_gate();
    let stdout = capture_stdout(|| {
        print_cached_summary(
            &paths,
            &cache,
            None,
            TimingLangInclude {
                python: true,
                rust: false,
            },
            &[],
            &gate,
        );
    });
    assert_nested_runtime_table(&stdout);
}

#[test]
fn print_summary_from_pipeline_forwards_resolved_gate_runtime_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = [tmp.path().to_string_lossy().into_owned()];
    let pipeline = crate::analyze::empty_full_pipeline_result_for_tests();
    let gate = nested_runtime_gate();
    let stdout = capture_stdout(|| {
        print_summary_from_pipeline(&paths, &pipeline, None, &[], &gate);
    });
    assert_nested_runtime_table(&stdout);
}

#[test]
fn runtime_section_helper_covers_defaults_and_disabled_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = [tmp.path().to_string_lossy().into_owned()];
    let include = TimingLangInclude {
        python: true,
        rust: false,
    };

    let disabled = GateConfig {
        max_unit_test_seconds: Vec::new(),
        ..GateConfig::default()
    };
    assert!(
        unit_test_runtime_section_for_rules(
            &paths,
            None,
            include,
            &[],
            &disabled.max_unit_test_seconds,
        )
        .is_none()
    );

    let defaults = GateConfig::default();
    let default_report = unit_test_runtime_section_for_rules(
        &paths,
        None,
        include,
        &[],
        &defaults.max_unit_test_seconds,
    )
    .expect("default catch-all should emit a runtime table");
    assert!(default_report.starts_with("unit_test_runtime_sec:"));
    assert!(default_report.contains("*\t2\t0\t-\t-\t-\t-\t-"));
}
