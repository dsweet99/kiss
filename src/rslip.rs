pub use ::rslip::*;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::test_refs::TestRefAnalysis;

pub fn query_covering_tests(
    repo_root: &Path,
    changed_sources: &[PathBuf],
) -> Result<Vec<crate::test_refs::CoveringTest>, String> {
    ::rslip::query_covering_tests(repo_root, changed_sources, pyfork::default_parallelism())
}

pub fn runtime_analysis_for_parsed(
    repo_root: &Path,
    static_analysis: &TestRefAnalysis,
) -> Result<TestRefAnalysis, String> {
    let collector = ::rslip::PytestTraceCollector;
    runtime_analysis_for_parsed_with_collector(
        repo_root,
        static_analysis,
        &|root, selectors, _j| collector.collect(root, selectors, _j),
        pyfork::default_parallelism(),
    )
}

fn runtime_analysis_for_parsed_with_collector<F>(
    repo_root: &Path,
    static_analysis: &TestRefAnalysis,
    collector: &F,
    j: usize,
) -> Result<TestRefAnalysis, String>
where
    F: Fn(&Path, &[String], usize) -> Result<Vec<::rslip::TestCoverageRun>, String>,
{
    let db = ::rslip::current_database(repo_root, collector, j)?;
    Ok(analysis_from_database(repo_root, static_analysis, &db))
}

pub fn analysis_from_database(
    repo_root: &Path,
    static_analysis: &TestRefAnalysis,
    db: &::rslip::Database,
) -> TestRefAnalysis {
    let mut by_file: HashMap<String, Vec<crate::test_refs::CodeDefinition>> = HashMap::new();
    for def in &static_analysis.definitions {
        by_file
            .entry(::rslip::normalize_path(repo_root, &def.file))
            .or_default()
            .push(def.clone());
    }
    let mut unreferenced = Vec::new();
    let mut coverage_map = HashMap::new();
    for (rel, defs) in by_file {
        let Some(file) = db.files.get(&rel) else {
            unreferenced.extend(defs);
            continue;
        };
        let Some(coverage) = &file.coverage else {
            unreferenced.extend(defs);
            continue;
        };
        if !coverage.missing_lines.is_empty()
            && let Some(anchor) = defs.iter().min_by_key(|def| def.line).cloned()
        {
            unreferenced.push(anchor);
        }
        let tests = db
            .source_to_covering_tests
            .get(&rel)
            .map(|selectors| {
                selectors
                    .iter()
                    .filter_map(|selector| selector.split_once("::"))
                    .map(|(path, id)| (repo_root.join(path), id.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !tests.is_empty() {
            for def in defs {
                coverage_map.insert((def.file, def.name), tests.clone());
            }
        }
    }
    TestRefAnalysis {
        definitions: static_analysis.definitions.clone(),
        test_references: static_analysis.test_references.clone(),
        call_references: static_analysis.call_references.clone(),
        unreferenced,
        coverage_map,
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    #[test]
    fn witness_query_covering_tests_symbol() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _ = query_covering_tests(tmp.path(), &[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
    use std::path::PathBuf;

    fn static_analysis_for(tmp: &std::path::Path, name: &str) -> TestRefAnalysis {
        TestRefAnalysis {
            definitions: vec![crate::test_refs::CodeDefinition {
                name: name.to_string(),
                kind: crate::units::CodeUnitKind::Function,
                file: tmp.join("a.py"),
                line: 1,
                containing_class: None,
            }],
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        }
    }

    #[test]
    fn analysis_from_database_maps_covering_tests_to_static_definition() {
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
                vec!["test_a.py::test_a".to_string()],
            )]),
        };
        let static_analysis = static_analysis_for(tmp.path(), "a");

        let runtime = analysis_from_database(tmp.path(), &static_analysis, &db);

        assert_eq!(
            runtime
                .coverage_map
                .get(&(tmp.path().join("a.py"), "a".to_string())),
            Some(&vec![(tmp.path().join("test_a.py"), "test_a".to_string())])
        );
    }

    #[test]
    fn runtime_analysis_flags_bind_only_missing_body() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rel = "a.py".to_string();
        let db = ::rslip::Database {
            schema_version: ::rslip::SCHEMA_VERSION,
            rslip_version: ::rslip::RSLIP_VERSION.to_string(),
            config_fingerprints: BTreeMap::new(),
            files: BTreeMap::from([(
                rel.clone(),
                ::rslip::FileRecord {
                    path: rel.clone(),
                    role: ::rslip::FileRole::Source,
                    content_digest: String::new(),
                    len: 0,
                    mtime_ns: 0,
                    coverage: Some(::rslip::CoverageMetadata {
                        executable_lines: vec![1, 2],
                        executed_lines: vec![1],
                        missing_lines: vec![2],
                        percent_covered: 50,
                    }),
                },
            )]),
            tests: BTreeMap::new(),
            source_to_covering_tests: BTreeMap::from([(
                rel.clone(),
                vec!["test_a.py::test_bind_only".to_string()],
            )]),
        };
        let static_analysis = static_analysis_for(tmp.path(), "a");
        let runtime = analysis_from_database(tmp.path(), &static_analysis, &db);
        assert_eq!(runtime.unreferenced.len(), 1);
        assert!(
            runtime
                .coverage_map
                .contains_key(&(tmp.path().join("a.py"), "a".to_string()))
        );
    }

    #[test]
    fn analysis_from_database_marks_missing_runtime_file_unreferenced() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = ::rslip::Database {
            schema_version: ::rslip::SCHEMA_VERSION,
            rslip_version: ::rslip::RSLIP_VERSION.to_string(),
            config_fingerprints: BTreeMap::new(),
            files: BTreeMap::new(),
            tests: BTreeMap::new(),
            source_to_covering_tests: BTreeMap::new(),
        };
        let static_analysis = static_analysis_for(tmp.path(), "a");
        let runtime = analysis_from_database(tmp.path(), &static_analysis, &db);

        assert_eq!(runtime.definitions.len(), 1);
        assert_eq!(runtime.unreferenced.len(), 1);
        assert_eq!(runtime.unreferenced[0].name, "a");
        assert!(runtime.coverage_map.is_empty());
    }

    #[test]
    fn runtime_analysis_for_parsed_loads_clean_runtime_database() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "def a():\n    return 1\n").unwrap();
        std::fs::write(tmp.path().join("test_a.py"), "def test_a():\n    pass\n").unwrap();
        let static_analysis = static_analysis_for(tmp.path(), "a");

        let db = ::rslip::refresh_with_collector(
            tmp.path(),
            &|_, selectors, _j| {
                assert_eq!(selectors, &["test_a.py::test_a".to_string()]);
                Ok(vec![::rslip::TestCoverageRun {
                    selector: "test_a.py::test_a".to_string(),
                    test_path: PathBuf::from("test_a.py"),
                    hits: BTreeMap::from([(
                        PathBuf::from("a.py"),
                        BTreeSet::from([1_usize, 2_usize]),
                    )]),
                }])
            },
            1,
        )
        .unwrap();
        ::rslip::write_database_atomic(tmp.path(), &db).unwrap();

        let runtime = runtime_analysis_for_parsed(tmp.path(), &static_analysis).unwrap();

        assert!(runtime.unreferenced.is_empty());
        assert!(
            runtime
                .coverage_map
                .contains_key(&(tmp.path().join("a.py"), "a".to_string()))
        );
    }
}
