use crate::common::list_full_check_cache_files;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn chmod(path: &std::path::Path, mode: u32) {
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms).unwrap();
}

fn run_python_check_all(repo: &Path, home: &Path) -> Output {
    kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg("--all")
        .arg(repo)
        .env("HOME", home)
        .output()
        .unwrap()
}

#[test]
fn check_all_cache_hit_replays_on_second_run() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();

    let out1 = run_python_check_all(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(
        stdout1.contains("Analyzed:"),
        "expected summary line. stdout:\n{stdout1}"
    );
    assert!(
        !list_full_check_cache_files(home.path()).is_empty(),
        "expected full-check cache file under HOME. stdout:\n{stdout1}"
    );

    let out2 = run_python_check_all(repo.path(), home.path());
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
fn check_all_cache_hit_survives_metadata_only_source_change() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();

    let out1 = run_python_check_all(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    let cache_files1 = list_full_check_cache_files(home.path());
    assert_eq!(
        cache_files1.len(),
        1,
        "expected one full-check cache file after cold run. stdout:\n{stdout1}"
    );

    fs::OpenOptions::new()
        .write(true)
        .open(&src)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(60))
        .unwrap();

    let out2 = run_python_check_all(repo.path(), home.path());
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    let cache_files2 = list_full_check_cache_files(home.path());

    assert_eq!(out2.status.code(), out1.status.code());
    assert_eq!(
        stdout2, stdout1,
        "metadata-only changes should replay cached output.\n--stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
    assert_eq!(
        cache_files2, cache_files1,
        "metadata-only changes should reuse the existing full-check cache file"
    );
}

#[test]
fn check_all_cache_invalidates_when_sources_unreadable() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();

    let out1 = run_python_check_all(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(!list_full_check_cache_files(home.path()).is_empty());

    chmod(&src, 0o000);

    let out2 = run_python_check_all(repo.path(), home.path());
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert_ne!(
        stdout2, stdout1,
        "unreadable sources must not replay cached output.\n--stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
}

#[test]
fn check_all_cache_invalidates_on_content_change() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();

    let out1 = run_python_check_all(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(!list_full_check_cache_files(home.path()).is_empty());

    // Change the file content, then make it unreadable.
    chmod(&src, 0o200); // write-only
    fs::write(&src, "def foo():\n    return 2\n").unwrap();
    chmod(&src, 0o000); // unreadable, so a cache miss will drop parsing and change output

    let out2 = run_python_check_all(repo.path(), home.path());

    // We don't require a failure (the analyzer may skip unreadable files), but we do require
    // that it did NOT incorrectly replay the stale cached output.
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert_ne!(
        stdout2, stdout1,
        "after source change, cached output must not be replayed.\n--stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
}

#[test]
fn check_all_cache_invalidates_on_same_size_content_change() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    let content1 = "x=1\n# a\n";
    let content2 = "x=1\ny=2\n";
    assert_eq!(content1.len(), content2.len());
    fs::write(&src, content1).unwrap();

    let out1 = run_python_check_all(repo.path(), home.path());
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(!list_full_check_cache_files(home.path()).is_empty());

    let mtime: SystemTime = fs::metadata(&src).unwrap().modified().unwrap();
    fs::write(&src, content2).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&src)
        .unwrap()
        .set_modified(mtime)
        .unwrap();

    let out2 = run_python_check_all(repo.path(), home.path());
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert_ne!(
        stdout2, stdout1,
        "same-size content change with preserved mtime must not replay stale cache.\n\
         --stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
}
