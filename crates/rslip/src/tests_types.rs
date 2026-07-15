use super::*;

#[test]
fn request_and_coverage_structs_expose_expected_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RslipRequest::witness(tmp.path());
    assert_eq!(req.nodeid, "test_sample.py::test_ok");
    assert_eq!(req.python_version, "3.12.0");
    assert_eq!(req.pytest_version, "8.0.0");
    assert!(req.cache_root.ends_with(".rslip_cache"));

    let coverage = LineCoverage::witness();
    assert_eq!(coverage.files["app.py"], BTreeSet::from([1, 2]));

    let outcome = RslipOutcome::witness();
    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(outcome.cache_status, CacheStatus::Hit);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.stdout, None);
    assert_eq!(outcome.stderr, None);
    assert_eq!(CacheStatus::witness_hit(), CacheStatus::Hit);
}
