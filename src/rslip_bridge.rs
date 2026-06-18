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
        let tests = tests_covering_file(db, &rel);
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
            if !tests.is_empty() {
                coverage_map.insert((file.path.clone(), name), tests.clone());
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
#[path = "rslip_bridge_inline_test.rs"]
mod tests;
