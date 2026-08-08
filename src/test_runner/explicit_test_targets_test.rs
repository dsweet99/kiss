//! Explicit PATH test-operand planning: named tests only, no prior-failure fan-out.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::cwd_test_lock;
use crate::test_runner::{TargetPlanKind, plan_target_selectors};

fn init_git_repo(root: &Path) {
    let mut cmd = kiss::scrubbed_git_command(root);
    assert!(cmd.arg("init").status().unwrap().success());
}

fn write_prior_failures(root: &Path, count: usize) {
    let kiss_dir = root.join(".kiss");
    fs::create_dir_all(&kiss_dir).unwrap();
    let mut records = String::from("[");
    for i in 0..count {
        if i > 0 {
            records.push(',');
        }
        records.push_str(&format!(
            r#"{{"language":"python","selector":"tests/other.py::test_prior_{i}","identity":{{"schema_version":"kiss-test-last-status-v1","tool_versions":{{"python":"3.12.0","pytest":"8.0.0"}},"test_args":[],"env":{{}}}}}}"#
        ));
    }
    records.push(']');
    let body = format!(
        r#"{{"schema_version":"kiss-test-last-status-v1","records":{records}}}"#
    );
    fs::write(kiss_dir.join("test_last_status.json"), body).unwrap();
}

#[test]
fn explicit_single_python_test_ignores_prior_failure_fanout() {
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_a.py"),
        "def test_x():\n    assert True\n",
    )
    .unwrap();
    write_prior_failures(tmp.path(), 100);

    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let planned = plan_target_selectors(
        TargetPlanKind::Targets(&["tests/test_a.py::test_x".into()]),
        &[],
        &[],
        &[],
        Some(kiss::Language::Python),
    );
    std::env::set_current_dir(orig).unwrap();
    let planned = planned.expect("plan explicit test");

    assert_eq!(planned.py_sel, vec!["tests/test_a.py::test_x".to_string()]);
    assert!(planned.rs_sel.is_empty());
    assert!(!planned.python_population_required);
    assert!(planned.python_prior_failure_selectors.is_empty());
    assert!(!planned.coverage_decision_engine_used);
    assert!(planned.skip_python_index_rebuild_after_selective);
}

#[test]
fn explicit_python_test_file_selects_only_that_file() {
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_a.py"),
        "def test_x():\n    assert True\n\ndef test_y():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        tests.join("test_b.py"),
        "def test_other():\n    assert True\n",
    )
    .unwrap();
    write_prior_failures(tmp.path(), 50);

    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let planned = plan_target_selectors(
        TargetPlanKind::Targets(&["tests/test_a.py".into()]),
        &[],
        &[],
        &[],
        Some(kiss::Language::Python),
    );
    std::env::set_current_dir(orig).unwrap();
    let planned = planned.expect("plan explicit test file");

    assert_eq!(
        planned.py_sel,
        vec![
            "tests/test_a.py::test_x".to_string(),
            "tests/test_a.py::test_y".to_string(),
        ]
    );
    assert!(planned.python_prior_failure_selectors.is_empty());
    assert!(!planned.python_population_required);
    assert!(!planned.coverage_decision_engine_used);
}
