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

#[test]
fn fail_closed_analysis_marks_each_parsed_file_unreferenced() {
    let tmp = tempfile::TempDir::new().unwrap();
    let files = vec![
        parsed(tmp.path().join("a.py")),
        parsed(tmp.path().join("b.py")),
    ];
    let analysis = super::fail_closed_py_analysis(&files);

    assert_eq!(analysis.definitions.len(), 2);
    assert_eq!(analysis.unreferenced.len(), 2);
    assert!(analysis.test_references.is_empty());
    assert!(analysis.call_references.is_empty());
    assert!(analysis.coverage_map.is_empty());
    for (def, file) in analysis.definitions.iter().zip(&files) {
        assert_eq!(def.name, "rslip_refresh_failed");
        assert_eq!(def.kind, CodeUnitKind::Module);
        assert_eq!(def.file, file.path);
        assert_eq!(def.line, 1);
        assert_eq!(def.containing_class, None);
    }
    assert_eq!(analysis.unreferenced.len(), analysis.definitions.len());
    for (missing, def) in analysis.unreferenced.iter().zip(&analysis.definitions) {
        assert_eq!(missing.name, def.name);
        assert_eq!(missing.file, def.file);
        assert_eq!(missing.line, def.line);
    }
}

#[test]
fn runtime_py_analysis_fails_closed_when_refresh_cannot_run() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = tmp.path().join("missing-repo");
    let files = vec![parsed(repo.join("pkg/a.py"))];

    let analysis = runtime_py_analysis(&repo, &files, Some(1));

    assert_eq!(analysis.definitions.len(), 1);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.definitions[0].name, "rslip_refresh_failed");
    assert_eq!(analysis.unreferenced[0].file, repo.join("pkg/a.py"));
    assert!(analysis.test_references.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn runtime_py_analysis_fails_closed_for_every_parsed_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = tmp.path().join("missing-repo");
    let files = vec![parsed(repo.join("pkg/a.py")), parsed(repo.join("pkg/b.py"))];

    let analysis = runtime_py_analysis(&repo, &files, Some(1));

    assert_eq!(analysis.definitions.len(), files.len());
    assert_eq!(analysis.unreferenced.len(), files.len());
    for (def, file) in analysis.definitions.iter().zip(&files) {
        assert_eq!(def.name, "rslip_refresh_failed");
        assert_eq!(def.file, file.path);
        assert_eq!(def.line, 1);
    }
    for (missing, file) in analysis.unreferenced.iter().zip(&files) {
        assert_eq!(missing.name, "rslip_refresh_failed");
        assert_eq!(missing.file, file.path);
        assert_eq!(missing.line, 1);
    }
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn fail_closed_analysis_preserves_file_order_and_denied_units() {
    let tmp = tempfile::TempDir::new().unwrap();
    let files = vec![
        parsed(tmp.path().join("pkg/first.py")),
        parsed(tmp.path().join("pkg/second.py")),
        parsed(tmp.path().join("pkg/third.py")),
    ];

    let analysis = fail_closed_py_analysis(&files);

    let definition_files: Vec<_> = analysis
        .definitions
        .iter()
        .map(|def| def.file.strip_prefix(tmp.path()).unwrap().to_path_buf())
        .collect();
    let unreferenced_files: Vec<_> = analysis
        .unreferenced
        .iter()
        .map(|def| def.file.strip_prefix(tmp.path()).unwrap().to_path_buf())
        .collect();
    assert_eq!(
        definition_files,
        vec![
            PathBuf::from("pkg/first.py"),
            PathBuf::from("pkg/second.py"),
            PathBuf::from("pkg/third.py"),
        ]
    );
    assert_eq!(unreferenced_files, definition_files);
    assert!(
        analysis
            .definitions
            .iter()
            .all(|def| def.name == "rslip_refresh_failed" && def.line == 1)
    );
}

#[test]
fn fail_closed_analysis_preserves_duplicate_input_cardinality() {
    let tmp = tempfile::TempDir::new().unwrap();
    let duplicate_path = tmp.path().join("pkg/retried.py");
    let files = vec![
        parsed(duplicate_path.clone()),
        parsed(tmp.path().join("pkg/other.py")),
        parsed(duplicate_path.clone()),
    ];

    let analysis = fail_closed_py_analysis(&files);

    let definition_files: Vec<_> = analysis
        .definitions
        .iter()
        .map(|def| def.file.clone())
        .collect();
    let unreferenced_files: Vec<_> = analysis
        .unreferenced
        .iter()
        .map(|def| def.file.clone())
        .collect();
    assert_eq!(
        definition_files,
        vec![
            duplicate_path.clone(),
            tmp.path().join("pkg/other.py"),
            duplicate_path
        ]
    );
    assert_eq!(unreferenced_files, definition_files);
    assert!(
        analysis
            .unreferenced
            .iter()
            .all(|def| def.name == "rslip_refresh_failed" && def.kind == CodeUnitKind::Module)
    );
}

#[test]
fn fail_closed_analysis_keeps_empty_input_empty() {
    let analysis = fail_closed_py_analysis(&[]);

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn line_name_and_normalization_are_stable() {
    let root = Path::new("/repo");
    assert_eq!(line_name(17), "line_17");
    assert_eq!(
        normalize_against(root, Path::new("/repo/pkg/a.py")),
        "pkg/a.py"
    );
}
