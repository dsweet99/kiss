//! KPOP / plan check: nested inline `mod` bodies in `compute_rust_file_metrics` and `MetricStats::collect_rust`.
//! These tests verify the FIX for the nested mod gap described in plan.md.
use kiss::MetricStats;
use kiss::rust_fn_metrics::compute_rust_file_metrics;
use kiss::rust_parsing::parse_rust_file;
use std::io::Write;

#[test]
fn kpop_plan_nested_mod_file_metrics_matches_collect_rust() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r"mod m {{
    fn f() {{
        let _ = 1;
    }}
}}"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let fm = compute_rust_file_metrics(&parsed);
    let stats = MetricStats::collect_rust(&[&parsed]);

    assert_eq!(
        fm.statements, 1,
        "fix verified: file metrics now count statements in nested mod bodies"
    );
    assert_eq!(
        fm.functions, 1,
        "fix verified: file metrics now count functions in nested mod bodies"
    );
    assert_eq!(
        stats.statements_per_file,
        vec![1],
        "fix verified: statements_per_file includes nested mod"
    );
    assert_eq!(
        stats.functions_per_file,
        vec![1],
        "fix verified: functions_per_file includes nested mod"
    );
    assert!(
        !stats.statements_per_function.is_empty(),
        "collect_rust_from_items recurses into mod for per-function samples"
    );
}

#[test]
fn kpop_plan_cfg_test_mod_skipped_in_file_metrics() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r"#[cfg(test)]
mod t {{
    fn f() {{
        let _ = 1;
    }}
}}"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let fm = compute_rust_file_metrics(&parsed);
    let stats = MetricStats::collect_rust(&[&parsed]);

    assert_eq!(
        fm.statements, 0,
        "cfg(test) mod statements should NOT be counted in file metrics"
    );
    assert!(
        stats.statements_per_function.is_empty(),
        "cfg(test) mod: inner fn should be skipped in stats (consistent with file metrics)"
    );
}
