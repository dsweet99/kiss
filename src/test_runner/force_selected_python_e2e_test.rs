use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

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

fn force_gate() -> kiss::GateConfig {
    kiss::GateConfig {
        test_coverage_threshold: 0,
        orphan_detection: false,
        max_unit_test_seconds: Vec::new(),
        ..Default::default()
    }
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

fn run_forced_first_once(repo: &Path) -> String {
    let _py = crate::test_runner::TestEnvVarGuard::set("PYTHONDONTWRITEBYTECODE", "1");
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(repo).unwrap();
    let mut args = python_named_target_args("tests/test_pair.py::test_first", true);
    args.gate_config = force_gate();
    let out = capture_stdout(|| {
        assert_eq!(run_test(args), 0);
    });
    std::env::set_current_dir(orig).unwrap();
    out
}

fn persistent_force_python_repo() -> std::path::PathBuf {
    static REPO: OnceLock<std::path::PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-force-python-fixture");
        let marker = root.join(".kiss-force-primed");
        if marker.is_file() && root.join("tests/test_pair.py").is_file() {
            return root;
        }
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        write_two_python_tests(tmp.path());
        let status = std::process::Command::new("cp")
            .args([
                "-a",
                &format!("{}/.", tmp.path().display()),
                &format!("{}/", root.display()),
            ])
            .status()
            .expect("cp force python fixture");
        assert!(status.success());
        // Prime once during fixture init so the cache-bypass case is a single force run.
        let _ = run_forced_first_once(&root);
        fs::write(&marker, b"1").unwrap();
        root
    })
    .clone()
}

fn force_python_repo_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
#[cfg(unix)]
fn forced_explicit_python_target_runs_only_selected_selector() {
    let _lock = force_python_repo_lock();
    let _cwd = cwd_test_lock::lock();
    let repo = persistent_force_python_repo();
    let out = run_forced_first_once(&repo);
    assert_forced_selected_only(&out);
    assert!(
        out.contains("PASS:") && !out.contains("PASS (cached):"),
        "force run must execute fresh, got:\n{out}"
    );
}

#[test]
#[cfg(unix)]
fn forced_explicit_python_target_bypasses_cache_on_rerun() {
    let _lock = force_python_repo_lock();
    let _cwd = cwd_test_lock::lock();
    let repo = persistent_force_python_repo();
    let out = run_forced_first_once(&repo);
    assert_forced_selected_only(&out);
    assert!(
        out.contains("PASS:") && !out.contains("PASS (cached):"),
        "force run must bypass cache, got:\n{out}"
    );
}
