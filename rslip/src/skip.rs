use std::collections::BTreeMap;
use std::path::Path;

use crate::discovery::{config_fingerprints, discover_repo_files};
use crate::types::{Database, FileRecord};
use crate::util::content_digest;

pub fn is_file_dirty(cached: &FileRecord, current_mtime_ns: u128, current_digest: &str) -> bool {
    if cached.mtime_ns != current_mtime_ns {
        return true;
    }
    cached.content_digest != current_digest
}

#[allow(dead_code)]
pub fn file_dirty_mtime_diff_without_read(cached_mtime_ns: u128, current_mtime_ns: u128) -> bool {
    cached_mtime_ns != current_mtime_ns
}

#[allow(dead_code)]
pub fn file_dirty_same_mtime_different_digest(
    cached_mtime_ns: u128,
    current_mtime_ns: u128,
    cached_digest: &str,
    current_digest: &str,
) -> bool {
    cached_mtime_ns == current_mtime_ns && cached_digest != current_digest
}

fn test_file_dirty(db: &Database, files: &BTreeMap<String, FileRecord>, test_path: &str) -> bool {
    let Some(cached) = db.files.get(test_path) else {
        return true;
    };
    let Some(current) = files.get(test_path) else {
        return true;
    };
    is_file_dirty(cached, current.mtime_ns, &current.content_digest)
}

fn covered_file_dirty(
    db: &Database,
    files: &BTreeMap<String, FileRecord>,
    covered_path: &str,
) -> bool {
    let Some(cached) = db.files.get(covered_path) else {
        return true;
    };
    let Some(current) = files.get(covered_path) else {
        return true;
    };
    is_file_dirty(cached, current.mtime_ns, &current.content_digest)
}

fn config_fingerprints_match(db: &Database, files: &[FileRecord]) -> bool {
    let current = config_fingerprints(files);
    db.config_fingerprints == current
}

fn conftest_dirty(db: &Database, files: &BTreeMap<String, FileRecord>) -> bool {
    for (path, current) in files {
        if !path.ends_with("conftest.py") {
            continue;
        }
        match db.files.get(path) {
            None => return true,
            Some(cached) if is_file_dirty(cached, current.mtime_ns, &current.content_digest) => {
                return true;
            }
            Some(_) => {}
        }
    }
    db.files
        .keys()
        .any(|path| path.ends_with("conftest.py") && !files.contains_key(path))
}

pub fn scheduled_nodeids(
    repo_root: &Path,
    collected: &[String],
    db: &Database,
) -> Result<Vec<String>, String> {
    let file_records = discover_repo_files(repo_root)?;
    let files: BTreeMap<_, _> = file_records
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect();
    if !config_fingerprints_match(db, &file_records) || conftest_dirty(db, &files) {
        return Ok(collected.to_vec());
    }
    let mut scheduled = Vec::new();
    for nodeid in collected {
        if should_schedule_nodeid(db, &files, nodeid) {
            scheduled.push(nodeid.clone());
        }
    }
    Ok(scheduled)
}

fn should_schedule_nodeid(
    db: &Database,
    files: &BTreeMap<String, FileRecord>,
    nodeid: &str,
) -> bool {
    let Some(record) = db.tests.get(nodeid) else {
        return true;
    };
    if test_file_dirty(db, files, &record.test_path) {
        return true;
    }
    record
        .covered_files
        .iter()
        .any(|path| covered_file_dirty(db, files, path))
}

#[allow(dead_code)]
pub fn current_file_digest(path: &Path) -> Result<(u128, String), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok((mtime_ns, content_digest(&bytes)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::types::{FileRole, TestRecord};
    use crate::{RSLIP_VERSION, SCHEMA_VERSION};

    fn file(path: &str, digest: &str, mtime_ns: u128) -> FileRecord {
        FileRecord {
            path: path.to_string(),
            role: FileRole::Source,
            content_digest: digest.to_string(),
            len: 0,
            mtime_ns,
            coverage: None,
        }
    }

    fn test_record(nodeid: &str, test_path: &str, covered: &[&str]) -> TestRecord {
        TestRecord {
            selector: nodeid.to_string(),
            test_path: test_path.to_string(),
            content_digest: "td".into(),
            covered_files: covered.iter().map(|s| (*s).to_string()).collect(),
            covered_lines: BTreeMap::new(),
        }
    }

    fn db(tests: BTreeMap<String, TestRecord>, files: BTreeMap<String, FileRecord>) -> Database {
        Database {
            schema_version: SCHEMA_VERSION,
            rslip_version: RSLIP_VERSION.to_string(),
            config_fingerprints: BTreeMap::new(),
            files,
            tests,
            source_to_covering_tests: BTreeMap::new(),
        }
    }

    #[test]
    fn file_dirty_mtime_diff_without_read() {
        assert!(super::file_dirty_mtime_diff_without_read(1, 2));
        assert!(!super::file_dirty_mtime_diff_without_read(1, 1));
    }

    #[test]
    fn file_dirty_same_mtime_different_digest() {
        assert!(super::file_dirty_same_mtime_different_digest(
            5, 5, "a", "b"
        ));
        assert!(!super::file_dirty_same_mtime_different_digest(
            5, 5, "a", "a"
        ));
    }

    #[test]
    fn skip_schedules_missing_entry() {
        let database = db(BTreeMap::new(), BTreeMap::new());
        let files = BTreeMap::new();
        assert!(should_schedule_nodeid(&database, &files, "t.py::test_a"));
    }

    #[test]
    fn skip_omits_clean_cached_nodeid() {
        let nodeid = "t.py::test_a";
        let mut tests = BTreeMap::new();
        tests.insert(nodeid.to_string(), test_record(nodeid, "t.py", &["s.py"]));
        let mut files = BTreeMap::new();
        files.insert("t.py".to_string(), file("t.py", "td", 1));
        files.insert("s.py".to_string(), file("s.py", "sd", 1));
        let database = db(tests, files.clone());
        assert!(!should_schedule_nodeid(&database, &files, nodeid));
    }

    #[test]
    fn skip_reruns_when_covered_source_dirty() {
        let nodeid = "t.py::test_a";
        let mut tests = BTreeMap::new();
        tests.insert(nodeid.to_string(), test_record(nodeid, "t.py", &["s.py"]));
        let mut cached_files = BTreeMap::new();
        cached_files.insert("t.py".to_string(), file("t.py", "td", 1));
        cached_files.insert("s.py".to_string(), file("s.py", "sd", 1));
        let database = db(tests, cached_files);
        let mut current_files = BTreeMap::new();
        current_files.insert("t.py".to_string(), file("t.py", "td", 1));
        current_files.insert("s.py".to_string(), file("s.py", "sd2", 2));
        assert!(should_schedule_nodeid(&database, &current_files, nodeid));
    }

    #[test]
    fn skip_reruns_when_test_file_missing_from_current_scan() {
        let nodeid = "t.py::test_a";
        let mut tests = BTreeMap::new();
        tests.insert(nodeid.to_string(), test_record(nodeid, "t.py", &["s.py"]));
        let mut cached_files = BTreeMap::new();
        cached_files.insert("t.py".to_string(), file("t.py", "td", 1));
        cached_files.insert("s.py".to_string(), file("s.py", "sd", 1));
        let database = db(tests, cached_files);
        let mut current_files = BTreeMap::new();
        current_files.insert("s.py".to_string(), file("s.py", "sd", 1));

        assert!(should_schedule_nodeid(&database, &current_files, nodeid));
    }

    #[test]
    fn skip_reruns_when_covered_file_missing_from_cache_or_current_scan() {
        let nodeid = "t.py::test_a";
        let mut tests = BTreeMap::new();
        tests.insert(nodeid.to_string(), test_record(nodeid, "t.py", &["s.py"]));
        let mut cached_files = BTreeMap::new();
        cached_files.insert("t.py".to_string(), file("t.py", "td", 1));
        let database = db(tests.clone(), cached_files);
        let mut current_files = BTreeMap::new();
        current_files.insert("t.py".to_string(), file("t.py", "td", 1));
        current_files.insert("s.py".to_string(), file("s.py", "sd", 1));
        assert!(should_schedule_nodeid(&database, &current_files, nodeid));

        let mut cached_files = BTreeMap::new();
        cached_files.insert("t.py".to_string(), file("t.py", "td", 1));
        cached_files.insert("s.py".to_string(), file("s.py", "sd", 1));
        let database = db(tests, cached_files);
        let mut current_files = BTreeMap::new();
        current_files.insert("t.py".to_string(), file("t.py", "td", 1));
        assert!(should_schedule_nodeid(&database, &current_files, nodeid));
    }

    #[test]
    fn skip_reruns_all_on_conftest_change() {
        let nodeid = "t.py::test_a";
        let mut tests = BTreeMap::new();
        tests.insert(nodeid.to_string(), test_record(nodeid, "t.py", &["s.py"]));
        let mut cached_files = BTreeMap::new();
        cached_files.insert("t.py".to_string(), file("t.py", "td", 1));
        cached_files.insert("s.py".to_string(), file("s.py", "sd", 1));
        let database = db(tests, cached_files);
        let mut current_files = BTreeMap::new();
        current_files.insert("t.py".to_string(), file("t.py", "td", 1));
        current_files.insert("s.py".to_string(), file("s.py", "sd", 1));
        current_files.insert("conftest.py".to_string(), file("conftest.py", "cd", 1));
        assert!(conftest_dirty(&database, &current_files));
    }

    #[test]
    fn skip_reruns_all_when_cached_conftest_changes_or_disappears() {
        let mut cached_files = BTreeMap::new();
        cached_files.insert("conftest.py".to_string(), file("conftest.py", "old", 1));
        let database = db(BTreeMap::new(), cached_files);
        let mut current_files = BTreeMap::new();
        current_files.insert("conftest.py".to_string(), file("conftest.py", "new", 1));
        assert!(conftest_dirty(&database, &current_files));

        let current_files = BTreeMap::new();
        assert!(conftest_dirty(&database, &current_files));
    }

    #[test]
    fn current_file_digest_reports_mtime_and_content_digest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("a.py");
        std::fs::write(&path, "print('a')\n").unwrap();

        let (mtime_ns, digest) = current_file_digest(&path).unwrap();

        assert!(mtime_ns > 0);
        assert_eq!(digest, content_digest(b"print('a')\n"));
    }

    #[test]
    fn skip_reruns_all_on_config_fingerprint_change() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("pyproject.toml"), "[project]\nname='x'\n").unwrap();
        std::fs::write(tmp.path().join("t.py"), "def test_a():\n    pass\n").unwrap();
        let records = discover_repo_files(tmp.path()).unwrap();
        let fingerprints = config_fingerprints(&records);
        let mut database = db(BTreeMap::new(), BTreeMap::new());
        database.config_fingerprints = fingerprints;
        let collected = vec!["t.py::test_a".to_string()];
        std::fs::write(tmp.path().join("pyproject.toml"), "[project]\nname='y'\n").unwrap();
        let scheduled = scheduled_nodeids(tmp.path(), &collected, &database).unwrap();
        assert_eq!(scheduled, collected);
    }
}
