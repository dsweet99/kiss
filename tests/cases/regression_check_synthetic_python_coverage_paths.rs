use crate::common::{list_full_check_cache_files, seed_python_runtime_coverage};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn synthetic_python_runtime_coverage_paths_do_not_make_check_malformed() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    fs::write(repo.path().join("lib.py"), "VALUE = 1\n").unwrap();
    fs::write(
        repo.path().join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n\
\n\
[test]\n\
         test_coverage_threshold = 100\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    seed_python_runtime_coverage(
        repo.path(),
        &[(
            "test_lib.py::test_value",
            vec![
                ("lib.py", vec![1]),
                ("<frozen importlib._bootstrap>", vec![1]),
                (".kiss/rslip_cache/rslip_runtime.py", vec![1]),
            ],
        )],
    );

    let out = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("__coverage")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .current_dir(repo.path())
        .env("HOME", home.path())
        .output()
        .expect("kiss test should run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "synthetic coverage paths must be skipped, not treated as malformed\n\
         status: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code(),
    );
    assert!(
        !stdout.contains("malformed out-of-repository path"),
        "synthetic coverage paths leaked into malformed-path output:\n{stdout}"
    );
}

#[test]
fn seeded_python_runtime_coverage_becomes_stale_after_source_change() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    fs::write(repo.path().join("lib.py"), "VALUE = 1\n").unwrap();
    fs::write(
        repo.path().join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n\
\n\
[test]\n\
         test_coverage_threshold = 100\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    seed_python_runtime_coverage(
        repo.path(),
        &[("test_lib.py::test_value", vec![("lib.py", vec![1])])],
    );

    let first = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("__coverage")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .current_dir(repo.path())
        .env("HOME", home.path())
        .output()
        .expect("kiss test should run");
    assert!(
        first.status.success(),
        "fresh seeded coverage should pass before the source changes. stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    fs::write(repo.path().join("lib.py"), "VALUE = 1\nOTHER = 2\n").unwrap();
    let second = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("__coverage")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .current_dir(repo.path())
        .env("HOME", home.path())
        .output()
        .expect("kiss test should run");
    let stdout = String::from_utf8_lossy(&second.stdout);
    let stderr = String::from_utf8_lossy(&second.stderr);

    assert!(
        !second.status.success(),
        "stale seeded coverage should fail closed after the source changes"
    );
    assert!(
        stderr.contains("refreshing Python runtime coverage")
            && !stderr.contains("kiss test commit"),
        "stale coverage should trigger automatic Python refresh without the old manual hint. \
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn cold_python_check_refreshes_runtime_coverage_and_warm_check_reuses_cache() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
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
         orphan_module_enabled = false\n\
\n\
[test]\n\
         test_coverage_threshold = 100\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();

    let cold = run_python_check(&home, &repo);
    let cold_stdout = String::from_utf8_lossy(&cold.stdout);
    let cold_stderr = String::from_utf8_lossy(&cold.stderr);
    assert!(
        cold.status.success(),
        "cold cov should refresh coverage and pass. stdout:\n{cold_stdout}\nstderr:\n{cold_stderr}"
    );
    assert!(
        cold_stderr.contains("refreshing Python runtime coverage"),
        "cold cov should announce the automatic refresh. stdout:\n{cold_stdout}\nstderr:\n{cold_stderr}"
    );
    assert!(
        cold_stdout.contains("PASS: test_lib.py::test_value"),
        "cold cov should run the discovered Python population. stdout:\n{cold_stdout}"
    );
    assert!(
        list_full_check_cache_files(repo.path()).is_empty(),
        "cold cov must not write the static full-check cache"
    );

    let warm = run_python_check(&home, &repo);
    let warm_stdout = String::from_utf8_lossy(&warm.stdout);
    let warm_stderr = String::from_utf8_lossy(&warm.stderr);
    assert!(
        warm.status.success(),
        "warm cov should pass from coverage caches. stdout:\n{warm_stdout}\nstderr:\n{warm_stderr}"
    );
    assert!(
        !warm_stderr.contains("refreshing Python runtime coverage"),
        "warm cov should not refresh valid runtime coverage. stdout:\n{warm_stdout}\nstderr:\n{warm_stderr}"
    );
    assert!(
        !warm_stdout.contains("PASS:"),
        "warm cov should not run the Python population again. stdout:\n{warm_stdout}"
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
        "failing test should make cold cov fail. stdout:\n{failed_stdout}\nstderr:\n{failed_stderr}"
    );
    assert!(
        failed_stderr.contains("failed to refresh Python runtime line coverage")
            && failed_stderr.contains("population test run failed"),
        "failure should report refresh/test execution, not stale cache advice. \
         stdout:\n{failed_stdout}\nstderr:\n{failed_stderr}"
    );
    assert!(
        list_full_check_cache_files(repo.path()).is_empty(),
        "failed refresh must not publish a full-check cache"
    );

    write_refreshable_python_repo(&repo, "    assert value() == 1\n");
    let fixed = run_python_check(&home, &repo);
    let fixed_stdout = String::from_utf8_lossy(&fixed.stdout);
    let fixed_stderr = String::from_utf8_lossy(&fixed.stderr);
    assert!(
        fixed.status.success(),
        "fixed test should refresh and pass under cov. stdout:\n{fixed_stdout}\nstderr:\n{fixed_stderr}"
    );
    assert!(
        fixed_stderr.contains("refreshing Python runtime coverage")
            && fixed_stdout.contains("PASS: test_lib.py::test_value"),
        "after a failed refresh, the next valid cov should run and publish the population. \
         stdout:\n{fixed_stdout}\nstderr:\n{fixed_stderr}"
    );
    assert!(
        list_full_check_cache_files(repo.path()).is_empty(),
        "successful cov refresh must not publish the static full-check cache"
    );
}

#[test]
fn kiss_check_succeeds_when_tests_fail_while_cov_fails_refresh() {
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

    let cov = run_python_check(&home, &repo);
    let cov_stdout = String::from_utf8_lossy(&cov.stdout);
    let cov_stderr = String::from_utf8_lossy(&cov.stderr);
    assert!(
        !cov.status.success(),
        "kiss test must fail when the population tests fail.\nstdout:\n{cov_stdout}\nstderr:\n{cov_stderr}"
    );
    assert!(
        cov_stderr.contains("failed to refresh Python runtime line coverage")
            && cov_stderr.contains("population test run failed"),
        "kiss test must report the existing refresh/population failure.\nstdout:\n{cov_stdout}\nstderr:\n{cov_stderr}"
    );
}

fn write_refreshable_python_repo(repo: &TempDir, assertion: &str) {
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
         orphan_module_enabled = false\n\
\n\
[test]\n\
         test_coverage_threshold = 100\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
}

fn run_python_check(home: &TempDir, repo: &TempDir) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("__coverage")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .current_dir(repo.path())
        .env("HOME", home.path())
        .output()
        .expect("kiss test should run")
}
