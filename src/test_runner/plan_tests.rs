use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use super::*;
use crate::test_git::TestChangeMode;

fn git_in(dir: &Path) -> Command {
    crate::test_git::git_command(dir)
}

fn init(tmp: &TempDir) {
    assert!(git_in(tmp.path()).arg("init").status().unwrap().success());
    assert!(
        git_in(tmp.path())
            .args(["config", "user.email", "t@t.t"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["config", "user.name", "t"])
            .status()
            .unwrap()
            .success()
    );
}

fn commit_all(tmp: &TempDir, message: &str) {
    assert!(
        git_in(tmp.path())
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", message])
            .status()
            .unwrap()
            .success()
    );
}

fn discovered_files(tmp: &TempDir) -> BTreeMap<String, rslip::FileRecord> {
    rslip::discover_repo_files(tmp.path())
        .unwrap()
        .into_iter()
        .map(|file| (file.path.clone(), file))
        .collect()
}

fn write_rslip_database(tmp: &TempDir, tests: &[(&str, &[&str])]) {
    let files = discovered_files(tmp);
    let mut test_records = BTreeMap::new();
    let mut source_to_tests: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (selector, covered_files) in tests {
        let (test_path, _) = selector.split_once("::").unwrap();
        let test_file = files.get(test_path).unwrap();
        let covered = covered_files
            .iter()
            .map(|path| {
                source_to_tests
                    .entry((*path).to_string())
                    .or_default()
                    .insert((*selector).to_string());
                (*path).to_string()
            })
            .collect();
        test_records.insert(
            (*selector).to_string(),
            rslip::TestRecord {
                selector: (*selector).to_string(),
                test_path: test_path.to_string(),
                content_digest: test_file.content_digest.clone(),
                covered_files: covered,
                covered_lines: BTreeMap::new(),
            },
        );
    }
    let db = rslip::Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: rslip::config_fingerprints(
            &files.values().cloned().collect::<Vec<_>>(),
        ),
        files,
        tests: test_records,
        source_to_covering_tests: source_to_tests
            .into_iter()
            .map(|(source, tests)| (source, tests.into_iter().collect()))
            .collect(),
    };
    rslip::write_database_atomic(tmp.path(), &db).unwrap();
}

fn write_line_only_rslip_database(tmp: &TempDir) {
    let files = discovered_files(tmp);
    let file_records = files.values().cloned().collect::<Vec<_>>();
    let db = rslip::Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: rslip::config_fingerprints(&file_records),
        files,
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::new(),
    };
    rslip::write_database_atomic(tmp.path(), &db).unwrap();
}

#[test]
fn plan_selectors_commit_smoke_without_cache_errors() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init(&tmp);
    std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
    git_in(tmp.path()).args(["add", "."]).status().unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let result = plan_selectors(TestChangeMode::Commit, None, None, &[], None, None);
    std::env::set_current_dir(orig).unwrap();
    let planned = result.expect("empty Python collection should not require rslip cache");
    assert!(planned.py_sel.is_empty());
    assert!(planned.rs_sel.is_empty());
}

#[test]
fn run_test_returns_error_outside_git_repo() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let code = run_test(RunTestCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        extra: &[],
        ignore: &[],
        lang_filter: None,
        jobs: None,
        config_main_branch: None,
    });
    std::env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
}

#[test]
fn plan_selectors_rust_filter_does_not_require_rslip_cache() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init(&tmp);
    std::fs::write(tmp.path().join("lib.rs"), "pub fn f() {}\n").unwrap();
    git_in(tmp.path()).args(["add", "."]).status().unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let result = plan_selectors(
        TestChangeMode::Commit,
        None,
        None,
        &[],
        Some(Language::Rust),
        None,
    );

    std::env::set_current_dir(orig).unwrap();
    let planned = result.expect("rust-only planning should not load rslip cache");
    assert!(planned.py_sel.is_empty());
    assert!(planned.rs_sel.is_empty());
}

#[test]
fn plan_selectors_python_respects_ignored_cached_tests() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init(&tmp);
    std::fs::write(tmp.path().join("a.py"), "def value():\n    return 1\n").unwrap();
    std::fs::write(
        tmp.path().join("test_a.py"),
        "from a import value\n\ndef test_value():\n    assert value() == 1\n",
    )
    .unwrap();
    commit_all(&tmp, "baseline");
    write_rslip_database(&tmp, &[("test_a.py::test_value", &["a.py"])]);
    std::fs::write(tmp.path().join("a.py"), "def value():\n    return 2\n").unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let result = plan_selectors(
        TestChangeMode::Commit,
        None,
        None,
        &["test_a.py".to_string()],
        Some(Language::Python),
        None,
    );

    std::env::set_current_dir(orig).unwrap();
    let planned = result.unwrap();
    assert!(planned.py_sel.is_empty());
    assert!(planned.rs_sel.is_empty());
}

#[test]
fn plan_selectors_rejects_line_only_python_cache_without_running_tests() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init(&tmp);
    std::fs::write(tmp.path().join("a.py"), "def value():\n    return 1\n").unwrap();
    std::fs::write(
        tmp.path().join("test_a.py"),
        "def test_a():\n    raise AssertionError('planning must not execute pytest')\n",
    )
    .unwrap();
    commit_all(&tmp, "baseline");
    write_line_only_rslip_database(&tmp);
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let result = plan_selectors(
        TestChangeMode::Commit,
        None,
        None,
        &[],
        Some(Language::Python),
        None,
    );

    std::env::set_current_dir(orig).unwrap();
    let Err(err) = result else {
        panic!("line-only cache should be rejected before planning selectors");
    };
    assert!(
        err.contains(COLD_CACHE_MSG),
        "line-only caches should be rebuilt by kiss check, got: {err}"
    );
}

#[test]
fn plan_selectors_default_rust_only_repo_does_not_require_rslip_cache() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init(&tmp);
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn value() -> usize { 1 }\n",
    )
    .unwrap();
    commit_all(&tmp, "baseline");
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        concat!(
            "pub fn value() -> usize { 2 }\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    fn changed_rust_test() { assert_eq!(super::value(), 1); }\n",
            "}\n",
        ),
    )
    .unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let result = plan_selectors(TestChangeMode::Commit, None, None, &[], None, None);

    std::env::set_current_dir(orig).unwrap();
    let planned = result.unwrap();
    assert!(planned.py_sel.is_empty());
    assert_eq!(planned.rs_sel, vec!["tests::changed_rust_test"]);
}
