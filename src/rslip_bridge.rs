use crate::ParsedFile;
use crate::test_refs::{CodeDefinition, CoveringTest, TestRefAnalysis};
use crate::units::CodeUnitKind;
use rslip::Database;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub fn rslip_database_fingerprint(repo_root: &Path) -> String {
    let path = rslip::db_path(repo_root);
    match std::fs::read(&path) {
        Ok(bytes) => rslip::content_digest(&bytes),
        Err(_) => "MISSING".to_string(),
    }
}

fn has_line_coverage_database(db: &Database) -> bool {
    let mut source_records = db
        .files
        .values()
        .filter(|record| record.role == rslip::FileRole::Source)
        .peekable();
    source_records.peek().is_some() && source_records.all(|record| record.coverage.is_some())
}

fn has_test_mapping_database(db: &Database) -> bool {
    !db.tests.is_empty() && !db.source_to_covering_tests.is_empty()
}

fn load_current_coverage_database(repo_root: &Path) -> Option<(Database, Vec<String>, bool)> {
    let db = rslip::load_database(repo_root).ok().flatten()?;
    if !has_line_coverage_database(&db) {
        return None;
    }
    let changed = rslip::metadata_changed_files(repo_root, &db).ok()?;
    let has_test_mappings = has_test_mapping_database(&db);
    Some((db, changed, has_test_mappings))
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
    if let Some((db, changed, has_test_mappings)) = load_current_coverage_database(repo_root) {
        let (stale_all, stale_paths) = stale_inputs_for_parsed(&changed, repo_root, parsed);
        if !(has_test_mappings && stale_all) {
            return bridge_analysis_from_database_with_stale(
                repo_root,
                parsed,
                &db,
                stale_all,
                &stale_paths,
            );
        }
    }
    let j = jobs.unwrap_or_else(pyfork::default_parallelism);
    let collector = rslip::PytestTraceCollector;
    let collect = |root: &Path, selectors: &[String], parallelism: usize| {
        collector.collect(root, selectors, parallelism)
    };
    match rslip::refresh_and_store(repo_root, &collect, j) {
        Ok(db) => bridge_analysis_from_database(repo_root, parsed, &db),
        Err(err) => {
            eprintln!("error: rslip coverage refresh failed: {err}");
            fail_closed_py_analysis(parsed)
        }
    }
}

pub(crate) fn bridge_analysis_from_database(
    repo_root: &Path,
    parsed: &[ParsedFile],
    db: &Database,
) -> TestRefAnalysis {
    bridge_analysis_from_database_with_stale(repo_root, parsed, db, false, &HashSet::new())
}

fn stale_runtime_definition(file: &ParsedFile) -> CodeDefinition {
    CodeDefinition {
        name: "rslip_refresh_needed".to_string(),
        kind: CodeUnitKind::Module,
        file: file.path.clone(),
        line: 1,
        containing_class: None,
    }
}

fn bridge_analysis_from_database_with_stale(
    repo_root: &Path,
    parsed: &[ParsedFile],
    db: &Database,
    stale_all: bool,
    stale_paths: &HashSet<String>,
) -> TestRefAnalysis {
    let mut definitions = Vec::new();
    let mut unreferenced = Vec::new();
    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    for file in parsed {
        if crate::test_refs::py_init_marker_pct(file) == 100 {
            continue;
        }
        let rel = normalize_against(repo_root, &file.path);
        if stale_all || stale_paths.contains(&rel) {
            let def = stale_runtime_definition(file);
            definitions.push(def.clone());
            unreferenced.push(def);
            continue;
        }
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

fn stale_inputs_for_parsed(
    changed: &[String],
    repo_root: &Path,
    parsed: &[ParsedFile],
) -> (bool, HashSet<String>) {
    let parsed_rels = parsed
        .iter()
        .map(|file| normalize_against(repo_root, &file.path))
        .collect::<HashSet<_>>();
    let mut stale_paths = HashSet::new();
    for path in changed {
        if parsed_rels.contains(path) {
            stale_paths.insert(path.clone());
        } else {
            return (true, HashSet::new());
        }
    }
    (false, stale_paths)
}

fn fail_closed_py_analysis(parsed: &[ParsedFile]) -> TestRefAnalysis {
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
#[path = "rslip_bridge_fail_closed_test.rs"]
mod fail_closed_tests;
#[cfg(test)]
#[path = "rslip_bridge_inline_test.rs"]
mod tests;
#[cfg(test)]
#[path = "rslip_bridge_warm_cache_test.rs"]
mod warm_cache_tests;
