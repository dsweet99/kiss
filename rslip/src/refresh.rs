use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::coverage::{executable_lines_from_source, line_coverage};
use crate::database::{load_database, write_database_atomic};
use crate::discovery::{config_fingerprints, discover_repo_files};
use crate::skip::is_file_dirty;
use crate::types::{
    CoveringTest, Database, FileRecord, FileRole, PytestTraceCollector, TestRecord,
};
use crate::util::normalize_path;
use crate::{RSLIP_VERSION, SCHEMA_VERSION};

type CollectorFn<'a> =
    dyn Fn(&Path, &[String], usize) -> Result<Vec<crate::TestCoverageRun>, String> + 'a;

pub fn refresh_with_collector(
    repo_root: &Path,
    collector: &CollectorFn<'_>,
    j: usize,
) -> Result<Database, String> {
    let mut files = discover_repo_files(repo_root)?;
    let extra = coverage_refresh_pytest_extra(repo_root);
    let nodeids = pyfork::collect_nodeids(repo_root, &extra)?;
    refresh_selected_with_collector(repo_root, collector, &mut files, nodeids, j)
}

pub(crate) fn coverage_refresh_pytest_extra(repo_root: &Path) -> Vec<String> {
    if repo_root.join("tests").join("fast").is_dir() {
        return vec!["tests/fast".to_string()];
    }
    Vec::new()
}

fn refresh_selected_with_collector(
    repo_root: &Path,
    collector: &CollectorFn<'_>,
    files: &mut [FileRecord],
    nodeids: Vec<String>,
    j: usize,
) -> Result<Database, String> {
    let runs = collector(repo_root, &nodeids, j)?;
    let mut source_to_tests: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut test_records = BTreeMap::new();

    for run in runs {
        let (selector, record) = test_record_from_run(repo_root, files, &mut source_to_tests, run);
        test_records.insert(selector, record);
    }

    apply_coverage_from_tests(repo_root, files, &test_records);
    let file_map: BTreeMap<_, _> = files
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect();
    let source_to_covering_tests = source_to_tests
        .into_iter()
        .map(|(source, tests)| (source, tests.into_iter().collect()))
        .collect();
    Ok(Database {
        schema_version: SCHEMA_VERSION,
        rslip_version: RSLIP_VERSION.to_string(),
        config_fingerprints: config_fingerprints(files),
        files: file_map,
        tests: test_records,
        source_to_covering_tests,
    })
}

fn test_record_from_run(
    repo_root: &Path,
    files: &[FileRecord],
    source_to_tests: &mut BTreeMap<String, BTreeSet<String>>,
    run: crate::TestCoverageRun,
) -> (String, TestRecord) {
    let selector = run.selector.clone();
    let test_path = normalize_path(repo_root, &run.test_path);
    let mut covered_files = Vec::new();
    let mut covered_lines = BTreeMap::new();
    for (path, lines) in run.hits {
        let rel = normalize_path(repo_root, &path);
        let Some(record) = files
            .iter()
            .find(|file| file.path == rel && file.role == FileRole::Source)
        else {
            continue;
        };
        source_to_tests
            .entry(record.path.clone())
            .or_default()
            .insert(selector.clone());
        let mut lines_vec: Vec<_> = lines.iter().copied().collect();
        lines_vec.sort_unstable();
        covered_lines.insert(record.path.clone(), lines_vec);
        covered_files.push(record.path.clone());
    }
    covered_files.sort();
    covered_files.dedup();
    let digest = files
        .iter()
        .find(|file| file.path == test_path)
        .map_or_else(String::new, |file| file.content_digest.clone());
    let record = TestRecord {
        selector: selector.clone(),
        test_path,
        content_digest: digest,
        covered_files,
        covered_lines,
    };
    (selector, record)
}

fn apply_coverage_from_tests(
    repo_root: &Path,
    files: &mut [FileRecord],
    tests: &BTreeMap<String, TestRecord>,
) {
    let mut hits_by_source: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for test in tests.values() {
        for (source, lines) in &test.covered_lines {
            hits_by_source
                .entry(source.clone())
                .or_default()
                .extend(lines.iter().copied());
        }
    }
    for file in files {
        if file.role == FileRole::Source {
            let source = fs::read_to_string(repo_root.join(&file.path)).unwrap_or_default();
            let executable = executable_lines_from_source(&source);
            let executed = hits_by_source.remove(&file.path).unwrap_or_default();
            file.coverage = Some(line_coverage(&executable, &executed));
        }
    }
}

pub fn refresh_and_store(
    repo_root: &Path,
    collector: &CollectorFn<'_>,
    j: usize,
) -> Result<Database, String> {
    let db = refresh_with_collector(repo_root, collector, j)?;
    write_database_atomic(repo_root, &db)?;
    Ok(db)
}

pub fn changed_files(repo_root: &Path, db: &Database) -> Result<Vec<String>, String> {
    let current = discover_repo_files(repo_root)?;
    let mut changed = Vec::new();
    let current_fingerprints = config_fingerprints(&current);
    for (key, digest) in &current_fingerprints {
        if db.config_fingerprints.get(key) != Some(digest) {
            changed.push(key.clone());
        }
    }
    for key in db.config_fingerprints.keys() {
        if !current_fingerprints.contains_key(key) {
            changed.push(key.clone());
        }
    }
    let current_paths: HashSet<_> = current.iter().map(|file| file.path.as_str()).collect();
    for file in &current {
        match db.files.get(&file.path) {
            None => changed.push(file.path.clone()),
            Some(old) if is_file_dirty(old, file.mtime_ns, &file.content_digest) => {
                changed.push(file.path.clone());
            }
            Some(_) => {}
        }
    }
    changed.extend(
        db.files
            .keys()
            .filter(|path| !current_paths.contains(path.as_str()))
            .cloned(),
    );
    changed.sort();
    changed.dedup();
    Ok(changed)
}

pub fn refresh_changed_tests_with_collector(
    repo_root: &Path,
    db: &Database,
    changed_test_paths: &[PathBuf],
    collector: &CollectorFn<'_>,
    j: usize,
) -> Result<Database, String> {
    let mut files = discover_repo_files(repo_root)?;
    let changed: HashSet<_> = changed_test_paths
        .iter()
        .map(|path| normalize_path(repo_root, path))
        .collect();
    let extra = coverage_refresh_pytest_extra(repo_root);
    let nodeids: Vec<_> = pyfork::collect_nodeids(repo_root, &extra)?
        .into_iter()
        .filter(|nodeid| {
            nodeid
                .split_once("::")
                .is_some_and(|(path, _)| changed.contains(path))
        })
        .collect();
    let mut next = db.clone();
    for selector in &nodeids {
        next.tests.remove(selector);
    }
    for tests in next.source_to_covering_tests.values_mut() {
        tests.retain(|selector| next.tests.contains_key(selector));
    }
    let partial = refresh_selected_with_collector(repo_root, collector, &mut files, nodeids, j)?;
    next.config_fingerprints = partial.config_fingerprints;
    for (selector, record) in partial.tests {
        next.tests.insert(selector, record);
    }
    apply_coverage_from_tests(repo_root, &mut files, &next.tests);
    next.files = files
        .into_iter()
        .map(|file| (file.path.clone(), file))
        .collect();
    let mut source_to_tests: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (selector, record) in &next.tests {
        for source in &record.covered_files {
            source_to_tests
                .entry(source.clone())
                .or_default()
                .insert(selector.clone());
        }
    }
    next.source_to_covering_tests = source_to_tests
        .into_iter()
        .map(|(source, tests)| (source, tests.into_iter().collect()))
        .collect();
    Ok(next)
}

pub fn current_database(
    repo_root: &Path,
    collector: &CollectorFn<'_>,
    j: usize,
) -> Result<Database, String> {
    match load_database(repo_root)? {
        Some(db) if changed_files(repo_root, &db)?.is_empty() => Ok(db),
        _ => refresh_and_store(repo_root, collector, j),
    }
}

pub fn query_covering_tests(
    repo_root: &Path,
    changed_sources: &[PathBuf],
    j: usize,
) -> Result<Vec<CoveringTest>, String> {
    let collector = PytestTraceCollector;
    let collect = |root: &Path, selectors: &[String], parallelism: usize| {
        collector.collect(root, selectors, parallelism)
    };
    let db = current_database(repo_root, &collect, j)?;
    let mut out = BTreeSet::new();
    for source in changed_sources {
        let rel = normalize_path(repo_root, source);
        if let Some(selectors) = db.source_to_covering_tests.get(&rel) {
            for selector in selectors {
                if let Some((path, id)) = selector.split_once("::") {
                    out.insert((repo_root.join(path), id.to_string()));
                }
            }
        }
    }
    Ok(out.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::write_database_atomic;
    use crate::discovery::{config_fingerprints, discover_repo_files};
    use std::fs;
    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn query_covering_tests_uses_clean_database_selectors() {
        let tmp = TempDir::new().unwrap();
        write(&tmp.path().join("app.py"), "def app():\n    return 1\n");
        write(&tmp.path().join("other.py"), "def other():\n    return 2\n");
        write(
            &tmp.path().join("test_app.py"),
            "def test_app():\n    assert 1\n",
        );
        let file_records = discover_repo_files(tmp.path()).unwrap();
        let files = file_records
            .iter()
            .map(|file| (file.path.clone(), file.clone()))
            .collect();
        let db = Database {
            schema_version: SCHEMA_VERSION,
            rslip_version: RSLIP_VERSION.to_string(),
            config_fingerprints: config_fingerprints(&file_records),
            files,
            tests: BTreeMap::new(),
            source_to_covering_tests: BTreeMap::from([(
                "app.py".to_string(),
                vec![
                    "test_app.py::test_app".to_string(),
                    "selector_without_separator".to_string(),
                ],
            )]),
        };
        write_database_atomic(tmp.path(), &db).unwrap();

        let covering = query_covering_tests(
            tmp.path(),
            &[tmp.path().join("app.py"), tmp.path().join("other.py")],
            1,
        )
        .unwrap();
        for (path, id) in &covering {
            assert!(path.ends_with("test_app.py"));
            assert_eq!(id, "test_app");
        }
        assert_eq!(
            covering,
            vec![(tmp.path().join("test_app.py"), "test_app".to_string())]
        );
        let empty = query_covering_tests(tmp.path(), &[], 1).unwrap();
        assert!(empty.is_empty());
    }
}
