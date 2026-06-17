use std::process::Command;

use crate::support::git::init_git_repo;
use crate::support::kiss_test::{
    git_commit_all, kiss_bin, kiss_test_dry_run, pytest_lines, setup_warm_python_lib_repo,
    setup_warm_python_rust_mixed_repo, warm_rslip,
};

#[test]
fn kiss_test_cold_cache_exits_with_actionable_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let bin = kiss_bin();
    init_git_repo(tmp.path());
    std::fs::write(tmp.path().join("test_x.py"), "def test_x():\n    pass\n").unwrap();
    git_commit_all(tmp.path(), "init");
    let out = Command::new(bin)
        .current_dir(tmp.path())
        .args(["test", "commit"])
        .output()
        .expect("kiss test");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr.contains("run kiss check first to warm the rslip cache"),
        "stderr={stderr}"
    );
}

#[test]
fn kiss_test_dry_run_one_line_per_nodeid() {
    let tmp = setup_warm_python_lib_repo();
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 2\n").unwrap();
    let out = kiss_test_dry_run(tmp.path(), &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines = pytest_lines(&stdout);
    assert_eq!(lines.len(), 1, "unexpected stdout: {stdout}");
    assert!(stdout.contains("test_lib.py::test_f"), "stdout={stdout}");
}

#[test]
fn kiss_test_rejects_forbidden_pytest_extra() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::write(tmp.path().join("test_x.py"), "def test_x():\n    pass\n").unwrap();
    git_commit_all(tmp.path(), "init");
    warm_rslip(tmp.path());
    let out = Command::new(kiss_bin())
        .current_dir(tmp.path())
        .args(["test", "commit", "--", "--lf"])
        .output()
        .expect("kiss test");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("incompatible"),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn kiss_test_python_ignores_git_for_selection() {
    let tmp = setup_warm_python_rust_mixed_repo();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn bump(n: i32) -> i32 { n + 2 }\n",
    )
    .unwrap();
    let out = kiss_test_dry_run(tmp.path(), &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        pytest_lines(&stdout).is_empty(),
        "Python scheduling must use rslip skip cache, not git diff; stdout={stdout}"
    );
}

#[test]
fn kiss_test_rust_still_uses_git_selectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn test_add() { assert_eq!(add(1, 2), 3); }\n}\n",
    )
    .unwrap();
    git_commit_all(tmp.path(), "init");
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b + 1 }\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn test_add() { assert_eq!(add(1, 2), 4); }\n}\n",
    )
    .unwrap();
    let out = kiss_test_dry_run(tmp.path(), &["--lang", "rust"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout
            .lines()
            .any(|l| l.contains("cargo") && l.contains("test")),
        "Rust path should schedule via git diff; stdout={stdout}"
    );
}

#[test]
fn kiss_test_extra_args_repeated_per_fork() {
    let tmp = setup_warm_python_lib_repo();
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 2\n").unwrap();
    let out = kiss_test_dry_run(tmp.path(), &["--", "--tb=short"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines = pytest_lines(&stdout);
    assert_eq!(lines.len(), 1, "stdout={stdout}");
    assert!(
        lines[0].contains("--tb=short"),
        "extra pytest args must repeat on every fork; line={}",
        lines[0]
    );
}
