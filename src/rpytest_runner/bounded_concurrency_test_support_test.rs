use crate::rpytest_runner::bounded_concurrency_test_support::{
    CONCURRENCY_TEST_SCRIPT, ConcurrencyFixture, assert_passed_outcomes, concurrency_request,
    read_max_active, reset_concurrency_counters, setup_concurrency_fixture,
};
use crate::rpytest_runner::{PytestRunOutcome, TestStatus};

#[test]
fn concurrency_fixture_witness_and_fields_are_exercised() {
    let fixture = ConcurrencyFixture::witness();
    assert!(fixture.tmp.path().is_dir());
    assert!(fixture.state_path.is_file());
    assert!(fixture.max_path.is_file());
    assert!(fixture.env.contains_key("STATE_PATH"));
    assert!(fixture.env.contains_key("LOCK_PATH"));
}

#[test]
fn concurrency_fixture_declares_all_tests_at_module_scope() {
    for name in ["test_a", "test_b", "test_c", "test_d"] {
        assert!(
            CONCURRENCY_TEST_SCRIPT.contains(&format!("\ndef {name}():")),
            "{name} must be a top-level pytest function"
        );
    }
}

#[test]
fn concurrency_request_and_counter_helpers_round_trip() {
    let fixture = setup_concurrency_fixture();
    reset_concurrency_counters(&fixture.state_path, &fixture.max_path);
    assert_eq!(read_max_active(&fixture.max_path), 0);
    let req = concurrency_request(fixture.tmp.path(), &fixture.env, "test_sample.py::test_a");
    assert_eq!(req.nodeid, "test_sample.py::test_a");
}

#[test]
fn assert_passed_outcomes_accepts_passed_rows() {
    let outcomes = vec![Ok(PytestRunOutcome {
        nodeid: "n".to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        duration: std::time::Duration::ZERO,
        artifacts: std::collections::BTreeMap::new(),
    })];
    assert_passed_outcomes(&outcomes);
}
