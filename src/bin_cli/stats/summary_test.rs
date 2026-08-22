use super::*;
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
        .take_while(|l| l.split_whitespace().count() == 8)
        .collect();
    assert_eq!(rows.len(), 3, "rows:\n{}\nfull:\n{stdout}", rows.join("\n"));
    assert!(
        rows[0]
            .split_whitespace()
            .take(3)
            .eq(["tests/slow/dbs", "180", "0"])
    );
    assert!(
        rows[1]
            .split_whitespace()
            .take(3)
            .eq(["tests/slow", "60", "0"])
    );
    assert!(rows[2].split_whitespace().take(3).eq(["*", "0", "0"]));
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
fn runtime_section_helper_preserves_resolved_rule_order() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = [tmp.path().to_string_lossy().into_owned()];
    let gate = nested_runtime_gate();
    let report = unit_test_runtime_section_for_rules(
        &paths,
        None,
        TimingLangInclude {
            python: true,
            rust: false,
        },
        &[],
        &gate.max_unit_test_seconds,
    )
    .expect("configured rules should emit a runtime table");
    assert_nested_runtime_table(&report);
}

#[test]
fn runtime_section_helper_accepts_pipeline_language_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = [tmp.path().to_string_lossy().into_owned()];
    let pipeline = crate::analyze::empty_full_pipeline_result_for_tests();
    let gate = nested_runtime_gate();
    let report = unit_test_runtime_section_for_rules(
        &paths,
        None,
        TimingLangInclude {
            python: !pipeline.result.py_parsed.is_empty(),
            rust: !pipeline.result.rs_parsed.is_empty(),
        },
        &[],
        &gate.max_unit_test_seconds,
    )
    .expect("configured rules should emit a runtime table");
    assert_nested_runtime_table(&report);
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
    assert_eq!(
        format_violation_counts(1, 2, 3, 4),
        "Violations: 1 duplicate, 2 orphan, 3 comment, 4 doc"
    );
    let comment = kiss::Violation::builder("a.py").metric("comment").build();
    let doc = kiss::Violation::builder("b.py").metric("doc").build();
    assert_eq!(
        count_metric([comment.metric.as_str(), doc.metric.as_str()], "comment"),
        1
    );
    assert_eq!(
        count_metric([comment.metric.as_str(), doc.metric.as_str()], "doc"),
        1
    );
    assert!(default_report.starts_with("unit_test_runtime_sec:"));
    assert!(default_report.lines().any(|line| {
        line.split_whitespace()
            .eq(["*", "2", "0", "-", "-", "-", "-", "-"])
    }));
}
