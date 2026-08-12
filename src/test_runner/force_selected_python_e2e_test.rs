//! Forced explicit Python target must not widen into a full population.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::cwd_test_lock;
use crate::test_runner::capture_stdout::capture_stdout;
use crate::test_runner::python_named_target_args::python_named_target_args;
use crate::test_runner::run_test;

fn init_git_repo(root: &Path) {
    let mut cmd = kiss::scrubbed_git_command(root);
    assert!(cmd.arg("init").status().unwrap().success());
}

fn write_two_python_tests(root: &Path) {
    let tests = root.join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_pair.py"),
        "def test_first():\n    assert True\n\ndef test_second():\n    assert True\n",
    )
    .unwrap();
}

fn assert_forced_selected_only(stdout: &str) {
    assert!(
        !stdout.contains("kiss test: discovering python universe"),
        "force must not discover the python universe, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("kiss test: running python population"),
        "force must stay selective, got:\n{stdout}"
    );
    assert!(
        stdout.contains("test_pair.py::test_first"),
        "forced selector must appear, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("test_pair.py::test_second"),
        "sibling test must not run, got:\n{stdout}"
    );
}

#[test]
#[cfg(unix)]
fn forced_explicit_python_target_reruns_only_selected_selector() {
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_two_python_tests(tmp.path());

    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let first = capture_stdout(|| {
        assert_eq!(
            run_test(python_named_target_args(
                "tests/test_pair.py::test_first",
                true
            )),
            0
        );
    });
    assert_forced_selected_only(&first);
    assert!(
        first.contains("PASS:") && !first.contains("PASS (cached):"),
        "first force run must execute fresh, got:\n{first}"
    );

    let second = capture_stdout(|| {
        assert_eq!(
            run_test(python_named_target_args(
                "tests/test_pair.py::test_first",
                true
            )),
            0
        );
    });
    assert_forced_selected_only(&second);
    assert!(
        second.contains("PASS:") && !second.contains("PASS (cached):"),
        "second force run must bypass cache, got:\n{second}"
    );

    std::env::set_current_dir(orig).unwrap();
}
