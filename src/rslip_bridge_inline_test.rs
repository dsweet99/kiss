use super::*;
use std::collections::BTreeMap;

fn parsed(path: PathBuf) -> ParsedFile {
    let mut parser = crate::parsing::create_parser().unwrap();
    let source = "def a():\n    return 1\n".to_string();
    let tree = parser.parse(&source, None).unwrap();
    ParsedFile { path, source, tree }
}

fn parsed_source(path: PathBuf, source: &str) -> ParsedFile {
    let mut parser = crate::parsing::create_parser().unwrap();
    let tree = parser.parse(source, None).unwrap();
    ParsedFile {
        path,
        source: source.to_string(),
        tree,
    }
}

fn database_with_file(rel: &str, coverage: Option<rslip::CoverageMetadata>) -> Database {
    Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: BTreeMap::new(),
        files: BTreeMap::from([(
            rel.to_string(),
            rslip::FileRecord {
                path: rel.to_string(),
                role: rslip::FileRole::Source,
                content_digest: String::new(),
                len: 0,
                mtime_ns: 0,
                coverage,
            },
        )]),
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::new(),
    }
}

#[test]
fn rslip_database_fingerprint_reports_missing_database() {
    let tmp = tempfile::TempDir::new().unwrap();

    assert_eq!(rslip_database_fingerprint(tmp.path()), "MISSING");
}

#[test]
fn rslip_database_fingerprint_tracks_database_bytes() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join(".kiss")).unwrap();
    let db_path = rslip::db_path(tmp.path());
    std::fs::write(&db_path, b"first").unwrap();
    let first = rslip_database_fingerprint(tmp.path());

    std::fs::write(&db_path, b"second").unwrap();

    assert_ne!(first, "MISSING");
    assert_ne!(first, rslip_database_fingerprint(tmp.path()));
}

#[test]
fn runtime_py_analysis_empty_input_does_not_probe_database() {
    let tmp = tempfile::TempDir::new().unwrap();

    let analysis = runtime_py_analysis(&tmp.path().join("missing-repo"), &[], Some(1));

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.test_references.is_empty());
    assert!(analysis.call_references.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn analysis_from_database_maps_runtime_lines_to_module_defs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut db = database_with_file(
        "a.py",
        Some(rslip::CoverageMetadata {
            executable_lines: vec![1, 2],
            executed_lines: vec![1],
            missing_lines: vec![2],
            percent_covered: 50,
        }),
    );
    db.tests.insert(
        "tests/test_a.py::test_a".to_string(),
        rslip::TestRecord {
            selector: "tests/test_a.py::test_a".to_string(),
            test_path: "tests/test_a.py".to_string(),
            content_digest: String::new(),
            covered_files: vec!["a.py".to_string()],
            covered_lines: BTreeMap::new(),
        },
    );
    let analysis =
        bridge_analysis_from_database(tmp.path(), &[parsed(tmp.path().join("a.py"))], &db);

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
        panic!("executed lines should have covering tests");
    }
}

#[test]
fn runtime_py_analysis_reuses_current_full_database_before_refreshing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let source = "def value():\n    return 1\n";
    std::fs::write(tmp.path().join("a.py"), source).unwrap();
    std::fs::write(tmp.path().join("test_a.py"), "def broken(:\n    pass\n").unwrap();

    let records = rslip::discover_repo_files(tmp.path()).unwrap();
    let mut files: BTreeMap<_, _> = records
        .iter()
        .map(|record| (record.path.clone(), record.clone()))
        .collect();
    files.get_mut("a.py").unwrap().coverage = Some(rslip::CoverageMetadata {
        executable_lines: vec![1, 2],
        executed_lines: vec![1, 2],
        missing_lines: vec![],
        percent_covered: 100,
    });
    let db = Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: rslip::config_fingerprints(&records),
        files,
        tests: BTreeMap::from([(
            "test_a.py::test_value".to_string(),
            rslip::TestRecord {
                selector: "test_a.py::test_value".to_string(),
                test_path: "test_a.py".to_string(),
                content_digest: String::new(),
                covered_files: vec!["a.py".to_string()],
                covered_lines: BTreeMap::from([("a.py".to_string(), vec![1, 2])]),
            },
        )]),
        source_to_covering_tests: BTreeMap::from([(
            "a.py".to_string(),
            vec!["test_a.py::test_value".to_string()],
        )]),
    };
    rslip::write_database_atomic(tmp.path(), &db).unwrap();

    let analysis = runtime_py_analysis(
        tmp.path(),
        &[parsed_source(tmp.path().join("a.py"), source)],
        Some(1),
    );

    assert!(analysis.test_references.contains("test_a.py::test_value"));
    assert!(analysis.unreferenced.is_empty());
    assert!(
        analysis
            .coverage_map
            .contains_key(&(tmp.path().join("a.py"), "line_1".to_string()))
    );
}

#[test]
fn runtime_py_analysis_reuses_database_and_fails_closed_for_new_source() {
    let tmp = tempfile::TempDir::new().unwrap();
    let old_source = "def old_value():\n    return 1\n";
    let new_source = "def new_value():\n    return 2\n";
    std::fs::write(tmp.path().join("old.py"), old_source).unwrap();
    let records = rslip::discover_repo_files(tmp.path()).unwrap();
    let mut files: BTreeMap<_, _> = records
        .iter()
        .map(|record| (record.path.clone(), record.clone()))
        .collect();
    files.get_mut("old.py").unwrap().coverage = Some(rslip::CoverageMetadata {
        executable_lines: vec![1, 2],
        executed_lines: vec![1, 2],
        missing_lines: vec![],
        percent_covered: 100,
    });
    let db = Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: rslip::config_fingerprints(&records),
        files,
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::new(),
    };
    rslip::write_database_atomic(tmp.path(), &db).unwrap();
    std::fs::write(tmp.path().join("new.py"), new_source).unwrap();

    let analysis = runtime_py_analysis(
        tmp.path(),
        &[
            parsed_source(tmp.path().join("old.py"), old_source),
            parsed_source(tmp.path().join("new.py"), new_source),
        ],
        Some(1),
    );

    assert!(
        analysis
            .definitions
            .iter()
            .any(|def| def.file.ends_with("old.py") && def.name == "line_1")
    );
    assert!(
        analysis
            .unreferenced
            .iter()
            .any(|def| def.file.ends_with("new.py") && def.name == "rslip_refresh_needed")
    );
}

#[test]
fn runtime_check_fast_path_accepts_line_coverage_database() {
    let line_only = database_with_file(
        "a.py",
        Some(rslip::CoverageMetadata {
            executable_lines: vec![1],
            executed_lines: vec![],
            missing_lines: vec![1],
            percent_covered: 0,
        }),
    );

    assert!(has_line_coverage_database(&line_only));
}

#[test]
fn helper_formats_line_names_and_relative_paths() {
    let repo = Path::new("/tmp/repo");
    let line = line_name(42);
    let rel = normalize_against(repo, Path::new("/tmp/repo/pkg/a.py"));
    assert_eq!(line, "line_42");
    assert_eq!(rel, "pkg/a.py");
}

#[test]
fn missing_database_record_produces_no_runtime_definitions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: BTreeMap::new(),
        files: BTreeMap::new(),
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::new(),
    };

    let analysis =
        bridge_analysis_from_database(tmp.path(), &[parsed(tmp.path().join("a.py"))], &db);
    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn uncovered_file_without_covering_tests_emits_unreferenced_lines() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = database_with_file(
        "a.py",
        Some(rslip::CoverageMetadata {
            executable_lines: vec![1, 2],
            executed_lines: Vec::new(),
            missing_lines: vec![1, 2],
            percent_covered: 0,
        }),
    );

    let analysis =
        bridge_analysis_from_database(tmp.path(), &[parsed(tmp.path().join("a.py"))], &db);

    assert_eq!(analysis.definitions.len(), 2);
    assert_eq!(analysis.unreferenced.len(), 2);
    assert_eq!(
        analysis
            .unreferenced
            .iter()
            .map(|def| (def.name.as_str(), def.line))
            .collect::<Vec<_>>(),
        vec![("line_1", 1), ("line_2", 2)]
    );
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn import_free_init_marker_does_not_emit_runtime_line_definitions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = database_with_file(
        "pkg/__init__.py",
        Some(rslip::CoverageMetadata {
            executable_lines: vec![1],
            executed_lines: Vec::new(),
            missing_lines: vec![1],
            percent_covered: 0,
        }),
    );
    let init = parsed_source(
        tmp.path().join("pkg/__init__.py"),
        "\"\"\"Package marker for tests.\"\"\"\n",
    );

    let analysis = bridge_analysis_from_database(tmp.path(), &[init], &db);

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn init_with_imports_emits_runtime_line_definitions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = database_with_file(
        "pkg/__init__.py",
        Some(rslip::CoverageMetadata {
            executable_lines: vec![1],
            executed_lines: Vec::new(),
            missing_lines: vec![1],
            percent_covered: 0,
        }),
    );
    let init = parsed_source(tmp.path().join("pkg/__init__.py"), "import os\n");

    let analysis = bridge_analysis_from_database(tmp.path(), &[init], &db);

    assert_eq!(analysis.definitions.len(), 1);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.definitions[0].name, "line_1");
}

#[test]
fn tests_covering_file_parses_selector_path_and_name() {
    let mut db = database_with_file("a.py", None);
    db.tests.insert(
        "tests/test_a.py::TestA::test_line".to_string(),
        rslip::TestRecord {
            selector: "tests/test_a.py::TestA::test_line".to_string(),
            test_path: "tests/test_a.py".to_string(),
            content_digest: String::new(),
            covered_files: vec!["a.py".to_string()],
            covered_lines: BTreeMap::new(),
        },
    );

    let tests = tests_covering_file(&db, "a.py");
    assert_eq!(
        tests,
        vec![(
            PathBuf::from("tests/test_a.py"),
            "TestA::test_line".to_string()
        )]
    );
}
