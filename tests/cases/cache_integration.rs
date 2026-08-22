use crate::common::{generate_lockfile, list_full_check_cache_files, seed_python_runtime_coverage};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::SystemTime;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn chmod(path: &std::path::Path, mode: u32) {
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms).unwrap();
}

fn run_python_check(repo: &Path, home: &Path) -> Output {
    kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(repo)
        .env("HOME", home)
        .output()
        .unwrap()
}

#[test]
fn check_cache_hit_replays_on_second_run() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();
    seed_python_runtime_coverage(repo.path(), &[("test_simple.py::test_simple", vec![])]);

    let out1 = run_python_check(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(
        stdout1.contains("Analyzed:"),
        "expected summary line. stdout:\n{stdout1}"
    );
    assert!(
        !list_full_check_cache_files(repo.path()).is_empty(),
        "expected full-check cache file under repo/.kiss. stdout:\n{stdout1}"
    );

    let out2 = run_python_check(repo.path(), home.path());
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert_eq!(
        out2.status.code(),
        out1.status.code(),
        "exit status should match on cache hit.\n--stderr1--\n{}\n--stderr2--\n{}",
        String::from_utf8_lossy(&out1.stderr),
        String::from_utf8_lossy(&out2.stderr)
    );
    assert_eq!(
        stdout2, stdout1,
        "cache-hit output should match exactly.\n--stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
}

#[test]
fn check_cache_is_not_invalidated_when_runtime_coverage_changes() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();
    seed_python_runtime_coverage(repo.path(), &[("test_simple.py::test_simple", vec![])]);

    let out1 = run_python_check(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(
        stdout1.contains("Analyzed:"),
        "sanity: first run should report static analysis summary. stdout:\n{stdout1}"
    );
    assert!(!list_full_check_cache_files(repo.path()).is_empty());

    seed_python_runtime_coverage(
        repo.path(),
        &[(
            "test_simple.py::test_simple",
            vec![("simple.py", vec![1, 2])],
        )],
    );

    let out2 = run_python_check(repo.path(), home.path());
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert!(
        out2.status.success(),
        "static check should still pass after runtime coverage changes. stdout:\n{stdout2}\nstderr:\n{}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert_eq!(
        stdout2, stdout1,
        "unchanged source with changed runtime coverage should preserve the static check cache"
    );
}

#[test]
fn check_cache_invalidates_when_sources_unreadable() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();
    seed_python_runtime_coverage(repo.path(), &[("test_simple.py::test_simple", vec![])]);

    let out1 = run_python_check(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(!list_full_check_cache_files(repo.path()).is_empty());

    chmod(&src, 0o000);

    let out2 = run_python_check(repo.path(), home.path());
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert_ne!(
        stdout2, stdout1,
        "unreadable sources must not replay cached output.\n--stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
}

#[test]
fn check_cache_invalidates_on_mtime_or_size_change() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();
    seed_python_runtime_coverage(repo.path(), &[("test_simple.py::test_simple", vec![])]);

    let out1 = run_python_check(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(!list_full_check_cache_files(repo.path()).is_empty());

    chmod(&src, 0o200);
    fs::write(&src, "def foo():\n    return 2\n").unwrap();
    chmod(&src, 0o000);

    let out2 = run_python_check(repo.path(), home.path());

    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert_ne!(
        stdout2, stdout1,
        "after source change, cached output must not be replayed.\n--stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
}

#[test]
fn check_cache_invalidates_on_same_size_content_change() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    let content1 = "def foo():\n    return 1\n# padding!!\n";
    let content2 = "def a():\n    pass\ndef b():\n    pass\n";
    assert_eq!(content1.len(), content2.len());
    fs::write(&src, content1).unwrap();
    seed_python_runtime_coverage(repo.path(), &[("test_simple.py::test_simple", vec![])]);

    let out1 = run_python_check(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(!list_full_check_cache_files(repo.path()).is_empty());

    let mtime: SystemTime = fs::metadata(&src).unwrap().modified().unwrap();
    fs::write(&src, content2).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&src)
        .unwrap()
        .set_modified(mtime)
        .unwrap();

    let out2 = run_python_check(repo.path(), home.path());
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert_ne!(
        stdout2, stdout1,
        "same-size content change with preserved mtime must not replay stale cache.\n\
         --stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
}

fn run_mixed_cmd(home: &Path, repo: &Path, args: &[&str]) -> Output {
    let mut cmd = kiss_binary();
    cmd.arg("--defaults");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg(repo).env("HOME", home).output().unwrap()
}

fn sorted_stdout_lines(out: &Output) -> Vec<String> {
    let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    lines.sort_unstable();
    lines
}

fn write_mixed_workspace(repo: &Path) {
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("app.py"), "def py_value():\n    return 1\n").unwrap();
    fs::write(
        repo.join("test_app.py"),
        "from app import py_value\n\ndef test_py_value():\n    assert py_value() == 1\n",
    )
    .unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"mixed_cache\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        repo.join("src").join("lib.rs"),
        "pub fn rust_value() -> i32 {\n    1\n}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn rust_ok() {\n        assert_eq!(super::rust_value(), 1);\n    }\n}\n",
    )
    .unwrap();
    generate_lockfile(repo);
    seed_python_runtime_coverage(
        repo,
        &[("test_app.py::test_py_value", vec![("app.py", vec![1, 2])])],
    );
}

#[test]
fn mixed_workspace_cached_check_and_stats_match_uncached() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    write_mixed_workspace(repo.path());

    let (check1, check2) = replay_cmd(home.path(), repo.path(), &["check"]);
    assert_eq!(
        String::from_utf8_lossy(&check1.stdout),
        String::from_utf8_lossy(&check2.stdout),
        "cached kiss check must replay the uncached production dataset"
    );

    let (stats1, stats2) = replay_cmd(home.path(), repo.path(), &["stats", "--all"]);
    assert_eq!(
        sorted_stdout_lines(&stats1),
        sorted_stdout_lines(&stats2),
        "cached kiss stats must match the uncached production dataset"
    );
    assert_coverage_identity(home.path(), repo.path(), &["__coverage", "--all"]);
}

fn product_gate_lines(out: &Output) -> Vec<String> {
    sorted_stdout_lines(out)
        .into_iter()
        .filter(|line| {
            !line.starts_with("kiss test:")
                && !line.starts_with("PASS:")
                && !line.starts_with("FAIL:")
                && !line.starts_with("SKIP:")
        })
        .collect()
}

fn write_rust_inline_external_crate(repo: &Path) {
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::create_dir_all(repo.join("tests")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"role_cache_rs\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        repo.join("src").join("lib.rs"),
        "pub fn value() -> i32 {\n    1\n}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn inline_ok() {\n        assert_eq!(super::value(), 1);\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        repo.join("tests").join("external.rs"),
        "#[test]\nfn external_ok() {\n    assert_eq!(role_cache_rs::value(), 1);\n}\n",
    )
    .unwrap();
    generate_lockfile(repo);
}

fn replay_cmd(home: &Path, repo: &Path, args: &[&str]) -> (Output, Output) {
    let first = run_mixed_cmd(home, repo, args);
    let second = run_mixed_cmd(home, repo, args);
    assert_eq!(first.status.code(), second.status.code());
    (first, second)
}

fn assert_coverage_identity(home: &Path, repo: &Path, args: &[&str]) {
    let (cov1, cov2) = replay_cmd(home, repo, args);
    let cov1_lines = product_gate_lines(&cov1);
    assert!(
        cov1.status.success() && cov1_lines.iter().any(|line| line.contains("NO VIOLATIONS")),
        "coverage must succeed. stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&cov1.stdout),
        String::from_utf8_lossy(&cov1.stderr)
    );
    assert_eq!(
        cov1_lines,
        product_gate_lines(&cov2),
        "cached kiss test coverage must match the uncached production dataset"
    );
}

#[test]
fn python_only_cached_coverage_matches_uncached() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(repo.path().join("simple.py"), "def foo():\n    return 1\n").unwrap();
    fs::write(
        repo.path().join("test_simple.py"),
        "from simple import foo\n\ndef test_simple():\n    assert foo() == 1\n",
    )
    .unwrap();
    seed_python_runtime_coverage(
        repo.path(),
        &[(
            "test_simple.py::test_simple",
            vec![("simple.py", vec![1, 2])],
        )],
    );
    assert_coverage_identity(
        home.path(),
        repo.path(),
        &["__coverage", "--all", "--lang", "python"],
    );
}

#[test]
fn rust_inline_and_external_tests_cached_check_stats_and_coverage_match_uncached() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    write_rust_inline_external_crate(repo.path());

    let (check1, check2) = replay_cmd(home.path(), repo.path(), &["check", "--lang", "rust"]);
    let check1_out = String::from_utf8_lossy(&check1.stdout);
    assert!(
        check1.status.success() && check1_out.contains("Analyzed: 1 files"),
        "production check must omit test-only rust files. stdout:\n{check1_out}\nstderr:\n{}",
        String::from_utf8_lossy(&check1.stderr)
    );
    assert_eq!(
        check1_out.as_ref(),
        String::from_utf8_lossy(&check2.stdout).as_ref(),
        "cached kiss check must replay the uncached rust production dataset"
    );

    let (stats1, stats2) = replay_cmd(
        home.path(),
        repo.path(),
        &["stats", "--all", "--lang", "rust"],
    );
    assert_eq!(
        sorted_stdout_lines(&stats1),
        sorted_stdout_lines(&stats2),
        "cached kiss stats must match the uncached rust production dataset"
    );

    assert_coverage_identity(
        home.path(),
        repo.path(),
        &["__coverage", "--all", "--lang", "rust"],
    );
}
