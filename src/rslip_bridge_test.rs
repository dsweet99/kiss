use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::test_utils::parse_python_source;

fn parsed_with_path(path: PathBuf) -> crate::ParsedFile {
    let mut parsed = parse_python_source("def a():\n    return 1\n");
    parsed.path = path;
    parsed
}

fn bridge_db(
    coverage: Option<rslip::CoverageMetadata>,
    selector: &str,
    test_path: &str,
) -> rslip::Database {
    rslip::Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: BTreeMap::new(),
        files: BTreeMap::from([(
            "a.py".to_string(),
            rslip::FileRecord {
                path: "a.py".to_string(),
                role: rslip::FileRole::Source,
                content_digest: String::new(),
                len: 0,
                mtime_ns: 0,
                coverage,
            },
        )]),
        tests: BTreeMap::from([(
            selector.to_string(),
            rslip::TestRecord {
                selector: selector.to_string(),
                test_path: test_path.to_string(),
                content_digest: String::new(),
                covered_files: vec!["a.py".to_string()],
                covered_lines: BTreeMap::new(),
            },
        )]),
        source_to_covering_tests: BTreeMap::from([(
            "a.py".to_string(),
            vec![selector.to_string()],
        )]),
    }
}

#[test]
fn bridge_helpers_are_directly_callable_from_sibling_test_module() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = bridge_db(
        Some(rslip::CoverageMetadata {
            executable_lines: vec![1, 2],
            executed_lines: vec![1],
            missing_lines: vec![2],
            percent_covered: 50,
        }),
        "tests/test_a.py::test_a",
        "tests/test_a.py",
    );

    let line = crate::rslip_bridge::line_name(1);
    let rel = crate::rslip_bridge::normalize_against(tmp.path(), &tmp.path().join("a.py"));
    let analysis = crate::rslip_bridge::bridge_analysis_from_database(
        tmp.path(),
        &[parsed_with_path(tmp.path().join("a.py"))],
        &db,
    );

    assert_eq!(line, "line_1");
    assert_eq!(rel, "a.py");
    assert_eq!(analysis.definitions.len(), 2);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.unreferenced[0].name, "line_2");
    if let Some(tests) = analysis
        .coverage_map
        .get(&(tmp.path().join("a.py"), "line_1".to_string()))
    {
        assert_eq!(
            tests[0],
            (PathBuf::from("tests/test_a.py"), "test_a".to_string())
        );
    } else {
        panic!("executed runtime lines should map to covering tests");
    }
    assert_eq!(
        crate::rslip_bridge::normalize_against(Path::new("/tmp/repo"), Path::new("/tmp/repo/a.py")),
        "a.py"
    );
}

#[test]
fn bridge_ignores_records_without_coverage_metadata() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = bridge_db(None, "tests/test_a.py::test_a", "tests/test_a.py");

    let analysis = crate::rslip_bridge::bridge_analysis_from_database(
        tmp.path(),
        &[parsed_with_path(tmp.path().join("a.py"))],
        &db,
    );

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn bridge_falls_back_to_test_path_for_selector_without_separator() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = bridge_db(
        Some(rslip::CoverageMetadata {
            executable_lines: vec![1],
            executed_lines: vec![1],
            missing_lines: vec![],
            percent_covered: 100,
        }),
        "test_line",
        "tests/test_a.py",
    );

    let analysis = crate::rslip_bridge::bridge_analysis_from_database(
        tmp.path(),
        &[parsed_with_path(tmp.path().join("a.py"))],
        &db,
    );
    let tests = analysis
        .coverage_map
        .get(&(tmp.path().join("a.py"), "line_1".to_string()))
        .unwrap();

    assert_eq!(
        tests,
        &vec![(PathBuf::from("tests/test_a.py"), "test_line".to_string())]
    );
}
