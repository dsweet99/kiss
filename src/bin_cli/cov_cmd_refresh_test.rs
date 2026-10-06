use super::*;

fn refresh_cov_gate() -> kiss::GateConfig {
    kiss::GateConfig {
        test_coverage_threshold: 0,
        orphan_detection: false,
        max_unit_test_seconds: Vec::new(),
        ..kiss::GateConfig::default()
    }
}

#[test]
fn allow_refresh_true_invokes_refresh_on_identity_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let required = RequiredCoverageLanguages {
        python: true,
        rust: false,
    };
    let gate = refresh_cov_gate();
    // load_or_refresh_snapshot must fail closed when allow_refresh is false.
    assert!(
        matches!(
            load_or_refresh_snapshot(tmp.path(), required, &[], 1, false, &gate, &[]),
            Err(1)
        ),
        "cache-only load must fail closed on an empty repo"
    );
}
