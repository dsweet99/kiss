use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};

fn static_analysis(tmp: &std::path::Path, defs: &[(&str, usize)]) -> TestRefAnalysis {
    TestRefAnalysis {
        definitions: defs
            .iter()
            .map(|(name, line)| crate::test_refs::CodeDefinition {
                name: (*name).to_string(),
                kind: crate::units::CodeUnitKind::Function,
                file: tmp.join("a.py"),
                line: *line,
                containing_class: None,
            })
            .collect(),
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        unreferenced: Vec::new(),
        coverage_map: HashMap::new(),
    }
}

#[test]
fn analysis_from_database_keeps_fully_covered_file_without_covering_tests_referenced() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = ::rslip::Database {
        schema_version: ::rslip::SCHEMA_VERSION,
        rslip_version: ::rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: BTreeMap::new(),
        files: BTreeMap::from([(
            "a.py".to_string(),
            ::rslip::FileRecord {
                path: "a.py".to_string(),
                role: ::rslip::FileRole::Source,
                content_digest: String::new(),
                len: 0,
                mtime_ns: 0,
                coverage: Some(::rslip::CoverageMetadata {
                    executable_lines: vec![1, 2],
                    executed_lines: vec![1, 2],
                    missing_lines: Vec::new(),
                    percent_covered: 100,
                }),
            },
        )]),
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::new(),
    };
    let static_analysis = static_analysis(tmp.path(), &[("a", 1)]);

    let runtime = analysis_from_database(tmp.path(), &static_analysis, &db);

    assert!(runtime.unreferenced.is_empty());
    assert!(runtime.coverage_map.is_empty());
    assert_eq!(runtime.definitions.len(), 1);
    assert_eq!(runtime.definitions[0].name, static_analysis.definitions[0].name);
    assert_eq!(runtime.definitions[0].file, static_analysis.definitions[0].file);
    assert_eq!(runtime.definitions[0].line, static_analysis.definitions[0].line);
}

#[test]
fn analysis_from_database_ignores_malformed_covering_test_selectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = ::rslip::Database {
        schema_version: ::rslip::SCHEMA_VERSION,
        rslip_version: ::rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: BTreeMap::new(),
        files: BTreeMap::from([(
            "a.py".to_string(),
            ::rslip::FileRecord {
                path: "a.py".to_string(),
                role: ::rslip::FileRole::Source,
                content_digest: String::new(),
                len: 0,
                mtime_ns: 0,
                coverage: Some(::rslip::CoverageMetadata::default()),
            },
        )]),
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::from([(
            "a.py".to_string(),
            vec![
                "malformed_selector".to_string(),
                "tests/test_a.py::test_a".to_string(),
            ],
        )]),
    };
    let static_analysis = static_analysis(tmp.path(), &[("a", 1)]);

    let runtime = analysis_from_database(tmp.path(), &static_analysis, &db);

    assert_eq!(
        runtime
            .coverage_map
            .get(&(tmp.path().join("a.py"), "a".to_string())),
        Some(&vec![(tmp.path().join("tests/test_a.py"), "test_a".to_string())])
    );
}

#[test]
fn analysis_from_database_maps_tests_to_all_defs_in_same_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = ::rslip::Database {
        schema_version: ::rslip::SCHEMA_VERSION,
        rslip_version: ::rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: BTreeMap::new(),
        files: BTreeMap::from([(
            "a.py".to_string(),
            ::rslip::FileRecord {
                path: "a.py".to_string(),
                role: ::rslip::FileRole::Source,
                content_digest: String::new(),
                len: 0,
                mtime_ns: 0,
                coverage: Some(::rslip::CoverageMetadata::default()),
            },
        )]),
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::from([(
            "a.py".to_string(),
            vec!["tests/test_a.py::test_a".to_string()],
        )]),
    };
    let static_analysis = static_analysis(tmp.path(), &[("first", 1), ("second", 3)]);

    let runtime = analysis_from_database(tmp.path(), &static_analysis, &db);

    assert_eq!(runtime.coverage_map.len(), 2);
    assert_eq!(
        runtime
            .coverage_map
            .get(&(tmp.path().join("a.py"), "first".to_string())),
        runtime
            .coverage_map
            .get(&(tmp.path().join("a.py"), "second".to_string()))
    );
    assert!(runtime.unreferenced.is_empty());
}
