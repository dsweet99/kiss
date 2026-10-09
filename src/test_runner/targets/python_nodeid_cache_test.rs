use super::{lookup_python_file_nodeids, repo_relative, store_python_file_nodeids};
use std::fs;

#[test]
fn python_file_nodeid_cache_round_trips_and_misses_on_rewrite() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("tests").join("test_a.py");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"def test_a():\n    assert True\n").unwrap();

    assert!(store_python_file_nodeids(
        root,
        &[(file.clone(), vec!["tests/test_a.py::test_a".into()])]
    ));
    assert_eq!(
        lookup_python_file_nodeids(root, &file).as_deref(),
        Some(["tests/test_a.py::test_a".to_string()].as_slice())
    );

    fs::write(&file, b"def test_a():\n    assert True\n# touched\n").unwrap();
    assert!(lookup_python_file_nodeids(root, &file).is_none());
}

#[test]
fn repo_relative_keeps_symlink_leaf_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("impl.py"), b"def test_ok():\n    assert True\n").unwrap();
    std::os::unix::fs::symlink("impl.py", root.join("test_ok.py")).unwrap();
    assert_eq!(
        repo_relative(root, &root.join("test_ok.py")).as_deref(),
        Some("test_ok.py")
    );
}
