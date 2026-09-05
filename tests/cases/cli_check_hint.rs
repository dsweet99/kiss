use crate::common::seed_python_runtime_coverage;
use crate::support::git::{commit_all, init_git_repo};
use kiss::cli_output::VIOLATIONS_FIX_HINT;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn cli_check_default_gate_emits_hint_on_coverage_failure() {
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    fs::write(tmp.path().join("orphan.py"), "def orphan():\n    pass\n").unwrap();
    seed_python_runtime_coverage(
        tmp.path(),
        &[(
            "tests/test_orphan.py::test_orphan",
            vec![("orphan.py", vec![])],
        )],
    );
    let config = crate::common::write_builtin_language_config(tmp.path());
    commit_all(tmp.path(), "init");
    let output = kiss_binary()
        .current_dir(tmp.path())
        .arg("test")
        .arg("--config")
        .arg(&config)
        .arg(".")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("VIOLATION:test_coverage:"),
        "expected coverage violation. stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(VIOLATIONS_FIX_HINT),
        "coverage gate failure should include fix hint. stdout: {stdout}"
    );
}
