use super::{lookup_python_file_nodeids, store_python_file_nodeids};
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
