use super::{
    load_cached_rust_workspace_selectors, load_cached_workspace_selectors,
    store_rust_workspace_selectors, store_workspace_selectors,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn store_workspace_selectors_fails_closed_when_writes_fail() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();

    fs::write(root.join(".kiss"), "not-a-directory").unwrap();
    fs::write(root.join("target"), "not-a-directory").unwrap();
    assert!(
        store_workspace_selectors(root, &[], &["tests/test_a.py::test_a".into()], &[], &[])
            .is_none(),
        "unwritable cache parents must not report a successful fingerprint"
    );
    assert!(load_cached_workspace_selectors(root, &[], &[]).is_none());
}

#[test]
fn workspace_selector_cache_round_trips_then_misses_on_touch() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    let py = root.join("tests").join("test_a.py");
    fs::write(&py, "def test_a():\n    assert True\n").unwrap();
    let rs = root.join("lib.rs");
    fs::write(&rs, "#[test]\nfn t() {}\n").unwrap();

    store_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &["t".into()],
        &[],
    );
    assert!(
        root.join(".kiss")
            .join("python_test_selectors.json")
            .is_file()
    );
    assert!(
        root.join(".kiss")
            .join("rust_test_selectors.json")
            .is_file()
    );
    assert!(
        root.join(".kiss")
            .join("selector_source_digests.json")
            .is_file(),
        "per-file digest records must persist"
    );
    let hit = load_cached_workspace_selectors(root, &[], &[]).unwrap();
    assert_eq!(hit.0, vec!["tests/test_a.py::test_a".to_string()]);
    assert_eq!(hit.1, vec!["t".to_string()]);
    assert!(
        load_cached_workspace_selectors(root, &[], &["-q".into()]).is_none(),
        "python collection args must be part of the cache key"
    );
    fs::write(root.join("pytest.ini"), "[pytest]\n").unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_workspace_selectors(root, &[], &[]).is_none(),
        "pytest.ini must invalidate python discovery"
    );
    assert_eq!(
        load_cached_rust_workspace_selectors(root, &[]).as_deref(),
        Some(["t".to_string()].as_slice()),
        "python collection config must not drop rust selectors"
    );
    store_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &["t".into()],
        &[],
    );

    fs::write(&py, "def test_a():\n    assert True\n# touch\n").unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(load_cached_workspace_selectors(root, &[], &[]).is_none());
    assert_eq!(
        load_cached_rust_workspace_selectors(root, &[]).as_deref(),
        Some(["t".to_string()].as_slice()),
        "python content change must not drop rust selectors"
    );

    let same_len_before = fs::read(&rs).unwrap();
    let mut same_len_after = same_len_before.clone();
    if let Some(pos) = same_len_after.iter().position(|&b| b == b't') {
        same_len_after[pos] ^= 1;
    }
    assert_eq!(same_len_before.len(), same_len_after.len());
    fs::write(&rs, same_len_after).unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_rust_workspace_selectors(root, &[]).is_none(),
        "rust content change must miss rust selectors"
    );
}

#[test]
fn load_workspace_selectors_for_count_requires_matching_ignore() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    store_workspace_selectors(root, &[], &["tests/test_a.py::test_a".into()], &[], &[]);
    assert!(super::load_workspace_selectors_for_count(root, &["tests/slow".into()], &[]).is_none());
    assert!(super::load_workspace_selectors_for_count(root, &[], &[]).is_some());
}

#[test]
fn python_collection_args_are_part_of_cache_key() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    store_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &[],
        &["-q".into()],
    );
    assert!(load_cached_workspace_selectors(root, &[], &[]).is_none());
    let hit = load_cached_workspace_selectors(root, &[], &["-q".into()]).unwrap();
    assert_eq!(hit.0, vec!["tests/test_a.py::test_a".to_string()]);
}

#[test]
fn store_rust_selectors_does_not_clobber_different_ignore_cache() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    fs::write(root.join("lib.rs"), "#[test]\nfn t() {}\n").unwrap();
    store_workspace_selectors(
        root,
        &["src/main.rs".into()],
        &["tests/test_a.py::test_a".into()],
        &["t".into()],
        &[],
    );
    store_rust_workspace_selectors(root, &[], &["other".into()]);
    super::clear_rust_selector_memo_for_tests();
    let hit = load_cached_workspace_selectors(root, &["src/main.rs".into()], &[]).unwrap();
    assert_eq!(hit.0, vec!["tests/test_a.py::test_a".to_string()]);
    assert_eq!(hit.1, vec!["t".to_string()]);
    super::clear_rust_selector_memo_for_tests();
    fs::write(root.join("lib.rs"), "#[test]\nfn t() { let _ = 1; }\n").unwrap();
    let body_only = load_cached_workspace_selectors(root, &["src/main.rs".into()], &[]).unwrap();
    assert_eq!(body_only.1, vec!["t".to_string()]);
    fs::write(
        root.join("lib.rs"),
        "#[test]\nfn t() {}\n#[test]\nfn u() {}\n",
    )
    .unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_workspace_selectors(root, &["src/main.rs".into()], &[]).is_none(),
        "added #[test] must miss rust selector cache"
    );
}

#[test]
fn git_fingerprint_includes_untracked_sources() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let init = kiss::scrubbed_git_command(root)
        .args(["init"])
        .output()
        .unwrap();
    assert!(init.status.success(), "git init failed");
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let tracked = root.join("src").join("lib.rs");
    fs::write(&tracked, "pub fn a() {}\n").unwrap();
    let add = kiss::scrubbed_git_command(root)
        .args(["add", "src/lib.rs", ".gitignore"])
        .output()
        .unwrap();
    assert!(add.status.success(), "git add failed");
    let commit = kiss::scrubbed_git_command(root)
        .args([
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-m",
            "t",
        ])
        .output()
        .unwrap();
    assert!(commit.status.success(), "git commit failed");

    store_workspace_selectors(root, &[], &[], &["a".into()], &[]);
    assert!(load_cached_workspace_selectors(root, &[], &[]).is_some());

    fs::write(root.join("src").join("extra.rs"), "pub fn b() {}\n").unwrap();
    assert!(
        load_cached_workspace_selectors(root, &[], &[]).is_none(),
        "untracked .rs must miss selector cache"
    );
}
