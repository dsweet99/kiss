use super::timeout_for_selector;
use std::fs;
use std::time::Duration;

#[test]
fn batch_template_applies_per_selector_timeouts() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        r#"[test]
max_unit_test_seconds = [["tests/slow/dbs", 180], ["tests/allowed", 60], ["*", 0]]
"#,
    )
    .unwrap();
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let slow = timeout_for_selector(
        "tests/slow/dbs/test_vdb_scoped_integration.py::test_aremove_bulk_eckv_integration",
    );
    let allowed = timeout_for_selector("tests/allowed/test_foo.py::test_ok");
    let banned = timeout_for_selector("tests/fast/test_foo.py::test_ok");
    std::env::set_current_dir(previous).unwrap();
    assert_eq!(slow, Duration::from_secs(180));
    assert_eq!(allowed, Duration::from_secs(60));
    assert_eq!(banned, Duration::ZERO);
}

#[test]
fn nonzero_sla_does_not_become_pytest_wall_kill() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        r#"[test]
max_unit_test_seconds = [["tests/allowed", 60], ["*", 0]]
"#,
    )
    .unwrap();
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let allowed = timeout_for_selector("tests/allowed/test_foo.py::test_ok");
    std::env::set_current_dir(previous).unwrap();
    assert_eq!(allowed, Duration::from_secs(60));
}
