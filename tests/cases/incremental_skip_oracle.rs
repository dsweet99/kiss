use std::path::Path;
use std::process::Command;

use crate::support::git::init_git_repo;
use crate::support::kiss_test::{git_commit_all, kiss_bin, warm_rslip};

fn scheduled_from_dry_run(dir: &Path) -> Vec<String> {
    let out = Command::new(kiss_bin())
        .current_dir(dir)
        .args(["test", "commit", "--dry-run"])
        .output()
        .expect("kiss test --dry-run");
    assert!(
        out.status.success(),
        "kiss test dry-run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.contains("pytest") && line.contains("::"))
        .map(|line| {
            line.split_whitespace()
                .find(|tok| tok.contains("::"))
                .unwrap_or(line)
                .trim_matches('\'')
                .to_string()
        })
        .collect()
}

fn collected_nodeids(dir: &Path) -> Vec<String> {
    pyfork::collect_nodeids(dir, &[]).expect("pytest collection")
}

fn oracle_scheduled(dir: &Path, invalidate_all: bool, dirty_paths: &[&str]) -> Vec<String> {
    let collected = collected_nodeids(dir);
    if invalidate_all {
        return collected;
    }
    let db = rslip::load_database(dir)
        .expect("load db")
        .expect("warmed db");
    collected
        .into_iter()
        .filter(|nodeid| {
            let Some(record) = db.tests.get(nodeid) else {
                return true;
            };
            if dirty_paths.contains(&record.test_path.as_str()) {
                return true;
            }
            record
                .covered_files
                .iter()
                .any(|p| dirty_paths.contains(&p.as_str()))
        })
        .collect()
}

fn setup_warm_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 1\n").unwrap();
    std::fs::write(
        tmp.path().join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 1\n",
    )
    .unwrap();
    git_commit_all(tmp.path(), "init");
    warm_rslip(tmp.path());
    tmp
}

#[test]
fn oracle_source_edit() {
    let tmp = setup_warm_repo();
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 2\n").unwrap();
    let incremental = scheduled_from_dry_run(tmp.path());
    let oracle = oracle_scheduled(tmp.path(), false, &["lib.py"]);
    assert_eq!(incremental, oracle);
    assert!(incremental.iter().any(|id| id.contains("test_f")));
}

#[test]
fn oracle_test_file_edit() {
    let tmp = setup_warm_repo();
    std::fs::write(
        tmp.path().join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 1\n\ndef test_g():\n    pass\n",
    )
    .unwrap();
    let incremental = scheduled_from_dry_run(tmp.path());
    let oracle = oracle_scheduled(tmp.path(), false, &["test_lib.py"]);
    assert_eq!(incremental, oracle);
    assert!(!incremental.is_empty());
}

#[test]
fn oracle_conftest_edit() {
    let tmp = setup_warm_repo();
    std::fs::write(tmp.path().join("conftest.py"), "import pytest\n").unwrap();
    let incremental = scheduled_from_dry_run(tmp.path());
    let oracle = oracle_scheduled(tmp.path(), true, &[]);
    assert_eq!(incremental, oracle);
}

#[test]
fn oracle_config_edit() {
    let tmp = setup_warm_repo();
    std::fs::write(tmp.path().join("pytest.ini"), "[pytest]\n").unwrap();
    let incremental = scheduled_from_dry_run(tmp.path());
    let oracle = oracle_scheduled(tmp.path(), true, &[]);
    assert_eq!(incremental, oracle);
}

#[test]
fn oracle_dependency_only_change() {
    let tmp = setup_warm_repo();
    std::fs::write(tmp.path().join("requirements.txt"), "requests==2.0.0\n").unwrap();
    let incremental = scheduled_from_dry_run(tmp.path());
    assert!(
        incremental.is_empty(),
        "dependency-only change may skip until re-warm"
    );
}

#[test]
fn oracle_dependency_rewarm_after_check() {
    let tmp = setup_warm_repo();
    std::fs::write(tmp.path().join("requirements.txt"), "requests==2.0.0\n").unwrap();
    assert!(
        scheduled_from_dry_run(tmp.path()).is_empty(),
        "dependency-only change may skip until re-warm"
    );
    warm_rslip(tmp.path());
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 9\n").unwrap();
    let incremental = scheduled_from_dry_run(tmp.path());
    let oracle = oracle_scheduled(tmp.path(), false, &["lib.py"]);
    assert_eq!(incremental, oracle);
    assert!(!incremental.is_empty());
}
