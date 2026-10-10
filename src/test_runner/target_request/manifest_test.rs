use super::*;
use crate::test_runner::target_request::workspace_request;

#[test]
fn empty_repo_manifests_complete_and_cache_matches_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let req = workspace_request(None, &[]);
    assert!(manifests_complete(root, &req));
    assert!(cache_matches_inventory(root, &req, &[], &[]));
}

#[test]
fn python_tests_without_selector_cache_block_unless_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join("app.py"), "def foo():\n    return 1\n").unwrap();
    std::fs::create_dir_all(root.join("resources")).unwrap();
    std::fs::write(
        root.join("resources/fixture_test.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    let ignoring = workspace_request(Some(kiss::Language::Python), &["resources".to_string()]);
    assert!(manifests_complete(root, &ignoring));
    assert!(cache_matches_inventory(root, &ignoring, &[], &[]));
    let not_ignoring = workspace_request(Some(kiss::Language::Python), &[]);
    assert!(!manifests_complete(root, &not_ignoring));
    assert!(!cache_matches_inventory(root, &not_ignoring, &[], &[]));
}

#[test]
fn needed_langs_respects_lang_filter() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join("lib.rs"), "fn f() {}\n").unwrap();
    std::fs::write(root.join("mod.py"), "x = 1\n").unwrap();
    let rust_only = workspace_request(Some(kiss::Language::Rust), &[]);
    let (need_py, need_rs) = needed_langs(root, &rust_only);
    assert!(!need_py);
    // rust may or may not be needed depending on gather; just exercise both filters
    let _ = need_rs;
    let py_only = workspace_request(Some(kiss::Language::Python), &[]);
    let (need_py2, need_rs2) = needed_langs(root, &py_only);
    assert!(!need_rs2);
    let _ = need_py2;
}

#[test]
fn same_set_is_order_insensitive() {
    assert!(super::same_set(
        &["a".into(), "b".into()],
        &["b".into(), "a".into()]
    ));
    assert!(!super::same_set(&["a".into()], &["b".into()]));
}
