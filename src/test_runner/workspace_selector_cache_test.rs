use super::{load_cached_workspace_selectors, store_workspace_selectors};
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
        store_workspace_selectors(root, &[], &["tests/test_a.py::test_a".into()], &[]).is_none(),
        "unwritable cache parents must not report a successful fingerprint"
    );
    assert!(load_cached_workspace_selectors(root, &[]).is_none());
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

    store_workspace_selectors(root, &[], &["tests/test_a.py::test_a".into()], &["t".into()]);
    let hit = load_cached_workspace_selectors(root, &[]).unwrap();
    assert_eq!(hit.0, vec!["tests/test_a.py::test_a".to_string()]);
    assert_eq!(hit.1, vec!["t".to_string()]);

    fs::write(&py, "def test_a():\n    assert True\n# touch\n").unwrap();
    assert!(load_cached_workspace_selectors(root, &[]).is_none());
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
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-m", "t"])
        .output()
        .unwrap();
    assert!(commit.status.success(), "git commit failed");

    store_workspace_selectors(root, &[], &[], &["a".into()]);
    assert!(load_cached_workspace_selectors(root, &[]).is_some());


    fs::write(root.join("src").join("extra.rs"), "pub fn b() {}\n").unwrap();
    assert!(
        load_cached_workspace_selectors(root, &[]).is_none(),
        "untracked .rs must miss selector cache"
    );
}
