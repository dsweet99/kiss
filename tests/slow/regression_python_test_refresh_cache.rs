use crate::common::list_full_check_cache_files;
use crate::support::git::{commit_all, init_git_repo};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn cold_python_test_runs_population_and_warm_test_reuses_records() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    init_git_repo(repo.path());
    fs::write(repo.path().join("lib.py"), "def value():\n    return 1\n").unwrap();
    fs::write(
        repo.path().join("test_lib.py"),
        "from lib import value\n\ndef test_value():\n    assert value() == 1\n",
    )
    .unwrap();
    fs::write(
        repo.path().join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
\n\
[test]\n\
         orphan_detection = false\n\
         num_jobs = 1\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    commit_all(repo.path(), "init");

    let cold = run_python_check(&home, &repo);
    let cold_stdout = String::from_utf8_lossy(&cold.stdout);
    let cold_stderr = String::from_utf8_lossy(&cold.stderr);
    assert!(
        cold.status.success(),
        "cold kiss test should run and pass. stdout:\n{cold_stdout}\nstderr:\n{cold_stderr}"
    );
    assert!(
        cold_stdout.contains("PASS: test_lib.py::test_value"),
        "cold kiss test should run the discovered Python population. stdout:\n{cold_stdout}"
    );
    assert!(
        list_full_check_cache_files(repo.path()).is_empty(),
        "cold kiss test must not write the static full-check cache"
    );

    let warm = run_python_check(&home, &repo);
    let warm_stdout = String::from_utf8_lossy(&warm.stdout);
    let warm_stderr = String::from_utf8_lossy(&warm.stderr);
    assert!(
        warm.status.success(),
        "warm kiss test should pass. stdout:\n{warm_stdout}\nstderr:\n{warm_stderr}"
    );
    assert!(
        warm_stdout.contains("1 passed") && warm_stdout.contains("PASS: test_lib.py::test_value"),
        "warm kiss test should run the selected Python test. stdout:\n{warm_stdout}"
    );
}

#[test]
fn failed_python_check_refresh_does_not_publish_full_check_cache() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    write_refreshable_python_repo(&repo, "    assert value() == 2\n");

    let failed = run_python_check(&home, &repo);
    let failed_stdout = String::from_utf8_lossy(&failed.stdout);
    let failed_stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(
        !failed.status.success(),
        "failing test should make cold kiss test fail. stdout:\n{failed_stdout}\nstderr:\n{failed_stderr}"
    );
    assert!(
        failed_stdout.contains("FAIL: test_lib.py::test_value"),
        "failure should report the failing test. \
         stdout:\n{failed_stdout}\nstderr:\n{failed_stderr}"
    );
    assert!(
        list_full_check_cache_files(repo.path()).is_empty(),
        "failed refresh must not publish a full-check cache"
    );
}

#[test]
fn successful_python_check_refresh_does_not_publish_full_check_cache() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    write_refreshable_python_repo(&repo, "    assert value() == 1\n");

    let passed = run_python_check(&home, &repo);
    let stdout = String::from_utf8_lossy(&passed.stdout);
    let stderr = String::from_utf8_lossy(&passed.stderr);
    assert!(
        passed.status.success(),
        "passing test should run and pass. stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("PASS: test_lib.py::test_value"),
        "kiss test should run and pass. stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        list_full_check_cache_files(repo.path()).is_empty(),
        "successful kiss test must not publish the static full-check cache"
    );
}

#[test]
fn kiss_check_succeeds_when_tests_fail_while_kiss_test_fails() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    write_refreshable_python_repo(&repo, "    assert value() == 2\n");

    let check = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .current_dir(repo.path())
        .env("HOME", home.path())
        .output()
        .expect("kiss check should run");
    let check_stdout = String::from_utf8_lossy(&check.stdout);
    let check_stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        check.status.success(),
        "kiss check must succeed without running failing tests.\nstdout:\n{check_stdout}\nstderr:\n{check_stderr}"
    );
    assert!(
        !check_stdout.contains("PASS:")
            && !check_stdout.contains("FAIL:")
            && !check_stderr.contains("refreshing")
            && !check_stderr.contains("population test run failed"),
        "kiss check must not execute the test population.\nstdout:\n{check_stdout}\nstderr:\n{check_stderr}"
    );

    let test = run_python_check(&home, &repo);
    let test_stdout = String::from_utf8_lossy(&test.stdout);
    let test_stderr = String::from_utf8_lossy(&test.stderr);
    assert!(
        !test.status.success(),
        "kiss test must fail when the population tests fail.\nstdout:\n{test_stdout}\nstderr:\n{test_stderr}"
    );
    assert!(
        test_stdout.contains("FAIL: test_lib.py::test_value"),
        "kiss test must report the failing test.\nstdout:\n{test_stdout}\nstderr:\n{test_stderr}"
    );
}

fn write_refreshable_python_repo(repo: &TempDir, assertion: &str) {
    init_git_repo(repo.path());
    fs::write(repo.path().join("lib.py"), "def value():\n    return 1\n").unwrap();
    fs::write(
        repo.path().join("test_lib.py"),
        format!("from lib import value\n\ndef test_value():\n{assertion}"),
    )
    .unwrap();
    fs::write(
        repo.path().join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
\n\
[test]\n\
         orphan_detection = false\n\
         num_jobs = 1\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    commit_all(repo.path(), "init");
}

fn run_python_check(home: &TempDir, repo: &TempDir) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_build_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.arg("test")
        .arg("--lang")
        .arg("python")
        .arg("test_lib.py::test_value")
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg("--jobs")
        .arg("1")
        .output()
        .expect("kiss test should run")
}

#[test]
fn commit_target_reruns_python_population_after_source_edit() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    write_refreshable_python_repo(&repo, "    assert value() == 1\n");
    let warm = run_python_check(&home, &repo);
    assert!(warm.status.success(), "warm-up must pass");

    fs::write(
        repo.path().join("lib.py"),
        "def value():\n    return int(1)\n",
    )
    .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_build_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    let commit = cmd
        .args(["--lang", "python", "test", "commit"])
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("kiss test commit should run");
    let stdout = String::from_utf8_lossy(&commit.stdout);
    assert!(
        commit.status.success() && stdout.contains("PASS: test_lib.py::test_value"),
        "a changed Python source reruns every Python test under `commit`.\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&commit.stderr)
    );
}
