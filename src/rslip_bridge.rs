use crate::ParsedFile;
use crate::test_refs::{CodeDefinition, CoveringTest, TestRefAnalysis};
use crate::units::CodeUnitKind;
use rslip::{Database, PytestTraceCollector};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub fn rslip_database_fingerprint(repo_root: &Path) -> String {
    let path = rslip::db_path(repo_root);
    match std::fs::read(&path) {
        Ok(bytes) => rslip::content_digest(&bytes),
        Err(_) => "MISSING".to_string(),
    }
}

pub fn runtime_py_analysis(
    repo_root: &Path,
    parsed: &[ParsedFile],
    jobs: Option<usize>,
) -> TestRefAnalysis {
    if parsed.is_empty() {
        return TestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
    }
    let j = jobs.unwrap_or_else(pyfork::default_parallelism);
    let collector = PytestTraceCollector;
    match rslip::current_database(
        repo_root,
        &|root, selectors, parallelism| collector.collect(root, selectors, parallelism),
        j,
    ) {
        Ok(db) => analysis_from_database(repo_root, parsed, &db),
        Err(err) => {
            eprintln!("error: rslip coverage refresh failed: {err}");
            fail_closed_analysis(parsed)
        }
    }
}

pub(crate) fn analysis_from_database(
    repo_root: &Path,
    parsed: &[ParsedFile],
    db: &Database,
) -> TestRefAnalysis {
    let mut definitions = Vec::new();
    let mut unreferenced = Vec::new();
    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    for file in parsed {
        let rel = normalize_against(repo_root, &file.path);
        let Some(record) = db.files.get(&rel) else {
            continue;
        };
        let Some(meta) = record.coverage.as_ref() else {
            continue;
        };
        for line in &meta.executable_lines {
            let name = line_name(*line);
            let def = CodeDefinition {
                name: name.clone(),
                kind: CodeUnitKind::Module,
                file: file.path.clone(),
                line: *line,
                containing_class: None,
            };
            if meta.missing_lines.contains(line) {
                unreferenced.push(def.clone());
            }
            definitions.push(def);
            let tests = tests_covering_file(db, &rel);
            if !tests.is_empty() {
                coverage_map.insert((file.path.clone(), name), tests);
            }
        }
    }
    TestRefAnalysis {
        definitions,
        test_references: db.tests.keys().cloned().collect(),
        call_references: HashSet::new(),
        unreferenced,
        coverage_map,
    }
}

fn fail_closed_analysis(parsed: &[ParsedFile]) -> TestRefAnalysis {
    let mut definitions = Vec::new();
    for file in parsed {
        definitions.push(CodeDefinition {
            name: "rslip_refresh_failed".to_string(),
            kind: CodeUnitKind::Module,
            file: file.path.clone(),
            line: 1,
            containing_class: None,
        });
    }
    TestRefAnalysis {
        unreferenced: definitions.clone(),
        definitions,
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        coverage_map: HashMap::new(),
    }
}

fn tests_covering_file(db: &Database, rel: &str) -> Vec<CoveringTest> {
    db.tests
        .values()
        .filter(|test| test.covered_files.iter().any(|path| path == rel))
        .map(|test| {
            let (path, name) = test
                .selector
                .split_once("::")
                .unwrap_or((&test.test_path, &test.selector));
            (PathBuf::from(path), name.to_string())
        })
        .collect()
}

pub(crate) fn line_name(line: usize) -> String {
    format!("line_{line}")
}

pub(crate) fn normalize_against(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn parsed(path: PathBuf) -> ParsedFile {
        let mut parser = crate::parsing::create_parser().unwrap();
        let source = "def a():\n    return 1\n".to_string();
        let tree = parser.parse(&source, None).unwrap();
        ParsedFile { path, source, tree }
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
        let analysis = analysis_from_database(tmp.path(), &[parsed(tmp.path().join("a.py"))], &db);

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

        let analysis = analysis_from_database(tmp.path(), &[parsed(tmp.path().join("a.py"))], &db);
        assert!(analysis.definitions.is_empty());
        assert!(analysis.unreferenced.is_empty());
        assert!(analysis.coverage_map.is_empty());
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
        let analysis = fail_closed_analysis(&files);

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
    fn fail_closed_analysis_preserves_file_order_and_denied_units() {
        let tmp = tempfile::TempDir::new().unwrap();
        let files = vec![
            parsed(tmp.path().join("pkg/first.py")),
            parsed(tmp.path().join("pkg/second.py")),
            parsed(tmp.path().join("pkg/third.py")),
        ];

        let analysis = fail_closed_analysis(&files);

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

        let analysis = fail_closed_analysis(&files);

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
        let analysis = fail_closed_analysis(&[]);

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
}
