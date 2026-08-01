use super::{load_cached_workspace_selectors, store_workspace_selectors};
use std::fs;
use tempfile::tempdir;

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
