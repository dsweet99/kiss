use super::*;

#[test]
fn runtime_py_analysis_writes_test_mapping_for_warm_cache() {
    let tmp = tempfile::TempDir::new().unwrap();
    let source = "def value():\n    return 1\n";
    std::fs::write(tmp.path().join("a.py"), "def value():\n    return 1\n").unwrap();
    std::fs::write(
        tmp.path().join("test_a.py"),
        "from a import value\n\ndef test_value():\n    assert value() == 1\n",
    )
    .unwrap();
    let mut parser = crate::parsing::create_parser().unwrap();
    let file = ParsedFile {
        path: tmp.path().join("a.py"),
        source: source.to_string(),
        tree: parser.parse(source, None).unwrap(),
    };

    let analysis = runtime_py_analysis(tmp.path(), &[file], Some(1));
    let db = rslip::load_database(tmp.path()).unwrap().unwrap();

    assert!(db.tests.contains_key("test_a.py::test_value"));
    assert_eq!(
        db.source_to_covering_tests["a.py"],
        vec!["test_a.py::test_value".to_string()]
    );
    let coverage = db.files["a.py"].coverage.as_ref().unwrap();
    assert_eq!(coverage.executable_lines, vec![1, 2]);
    assert!(coverage.executed_lines.contains(&1));
    assert!(coverage.executed_lines.contains(&2));
    assert!(analysis.test_references.contains("test_a.py::test_value"));
    assert!(
        analysis
            .coverage_map
            .contains_key(&(tmp.path().join("a.py"), "line_1".to_string()))
    );
}
