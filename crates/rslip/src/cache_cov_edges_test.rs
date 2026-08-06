use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::cell::Cell;
use std::time::Duration;

use rpytest_runner::{PytestRunOutcome, PytestRunner, TestStatus};

use super::cache::{
    self, covered_file_digests, entry_is_reusable, load_reusable_rslip_cache_entry,
    rslip_cache_fingerprint, store_rslip_cache_entry,
};
use super::runtime;
use super::{CacheStatus, LineCoverage, Rslip, RslipOutcome, rslip_sample_request};

const SYNTHETIC_KEYS_PY: &str =
    include_str!("../../../tests/fake_python/rslip_cov_edges/synthetic_keys.py");

fn synthetic_key(name: &str) -> String {
    let prefix = format!("{name} = ");
    for line in SYNTHETIC_KEYS_PY.lines() {
        let trimmed = line.trim();
        let Some(rhs) = trimmed.strip_prefix(&prefix) else {
            continue;
        };
        let rhs = rhs.trim();
        if let Some(inner) = rhs.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            return inner.to_string();
        }
        if let Some(inner) = rhs.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            return inner.to_string();
        }
    }
    panic!("synthetic_keys.py missing assignment for {name}");
}

fn cov_edges_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fake_python/rslip_cov_edges")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn real_mod_path() -> PathBuf {
    cov_edges_dir().join("real_mod.py")
}

fn real_mod_key() -> String {
    real_mod_path().to_string_lossy().replace('\\', "/")
}

fn coverage_with(keys: &[String]) -> LineCoverage {
    let files = keys
        .iter()
        .map(|key| (key.clone(), BTreeSet::from([1])))
        .collect();
    LineCoverage { files }
}

fn assert_key_omitted(source_root: &Path, key: &str) {
    let coverage = coverage_with(&[key.to_string()]);
    let digests = covered_file_digests(source_root, "test_real_mod.py::test_marker", &coverage)
        .expect("synthetic-only coverage remains digestable");
    assert!(!digests.contains_key(key));
}

fn passed_outcome(nodeid: &str, coverage: LineCoverage) -> RslipOutcome {
    RslipOutcome {
        nodeid: nodeid.to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        coverage,
        cache_status: CacheStatus::MissStored,
        stdout: None,
        stderr: None,
    }
}

fn stage_real_mod(tmp: &Path) -> String {
    let staged = tmp.join("real_mod.py");
    fs::copy(real_mod_path(), &staged).unwrap();
    fs::write(
        tmp.join("test_real_mod.py"),
        "def test_marker():\n    assert True\n",
    )
    .unwrap();
    staged.to_string_lossy().replace('\\', "/")
}

fn mixed_runner(calls: Rc<Cell<usize>>, real_key: String, type_key: String) -> PytestRunner {
    PytestRunner::from_fn(move |req| {
        calls.set(calls.get() + 1);
        let path = req.artifacts[0].path.clone();
        let payload = format!(
            r#"{{"files":{{"{}":[1],"{}":[1]}}}}"#,
            real_key.replace('\\', "/"),
            type_key.replace('\\', "/")
        );
        fs::write(&path, payload).unwrap();
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Passed,
            exit_code: Some(0),
            stdout: b"out".to_vec(),
            stderr: b"err".to_vec(),
            duration: Duration::from_millis(3),
            artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
        })
    })
}

#[test]
fn synthetic_coverage_keys_are_omitted_from_digests() {
    let tmp = tempfile::tempdir().unwrap();
    let real = real_mod_key();
    assert_key_omitted(tmp.path(), &synthetic_key("TYPE_ABSOLUTE"));
    assert_key_omitted(tmp.path(), &synthetic_key("TYPE_RELATIVE"));
    assert_key_omitted(tmp.path(), &synthetic_key("FROZEN"));
    assert_key_omitted(tmp.path(), &synthetic_key("RSLIP_RUNTIME"));
    assert_key_omitted(tmp.path(), &synthetic_key("KISS_RUNTIME"));

    let mixed = coverage_with(&[real.clone(), synthetic_key("TYPE_ABSOLUTE")]);
    let digests = covered_file_digests(tmp.path(), "ignored.py::test_x", &mixed).unwrap();
    assert!(digests.contains_key(&real));
    assert!(!digests.contains_key(&synthetic_key("TYPE_ABSOLUTE")));
}

#[test]
fn mixed_real_and_type_key_digests_include_real_mod_only() {
    let real = real_mod_key();
    let type_key = synthetic_key("TYPE_ABSOLUTE");
    let nodeid = "tests/fake_python/rslip_cov_edges/test_real_mod.py::test_marker";
    let coverage = coverage_with(&[real.clone(), type_key.clone()]);
    let digests = covered_file_digests(&workspace_root(), nodeid, &coverage)
        .expect("mixed coverage should digest");
    assert!(digests.contains_key(&real));
    assert!(!digests.contains_key(&type_key));
    assert!(digests.contains_key("tests/fake_python/rslip_cov_edges/test_real_mod.py"));
}

#[test]
fn mixed_coverage_reuses_via_rslip_without_second_runner_call() {
    let tmp = tempfile::tempdir().unwrap();
    let real_key = stage_real_mod(tmp.path());
    let type_key = synthetic_key("TYPE_ABSOLUTE");
    let calls = Rc::new(Cell::new(0));
    let rslip = Rslip::new(mixed_runner(Rc::clone(&calls), real_key.clone(), type_key));
    let mut req = rslip_sample_request(tmp.path());
    req.nodeid = "test_real_mod.py::test_marker".to_string();

    let first = rslip.run_or_reuse(req.clone()).unwrap();
    fs::write(tmp.path().join("unrelated.py"), "z = 9\n").unwrap();
    let second = rslip.run_or_reuse(req).unwrap();

    assert_eq!(first.cache_status, CacheStatus::MissStored);
    assert_eq!(second.cache_status, CacheStatus::Hit);
    assert!(first.coverage.files.contains_key(&real_key));
    assert_eq!(calls.get(), 1);
}

#[test]
fn editing_real_mod_invalidates_mixed_coverage_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let real_key = stage_real_mod(tmp.path());
    let type_key = synthetic_key("TYPE_RELATIVE");
    let coverage = coverage_with(&[real_key, type_key]);
    let nodeid = "test_real_mod.py::test_marker";
    let entry = cache::RslipCacheEntry::from_outcome(&passed_outcome(nodeid, coverage), tmp.path());
    assert!(entry_is_reusable(&entry, tmp.path()));
    fs::write(tmp.path().join("real_mod.py"), "def marker():\n    return 2\n").unwrap();
    assert!(!entry_is_reusable(&entry, tmp.path()));
}

#[test]
fn missing_real_covered_file_is_not_reusable() {
    let tmp = tempfile::tempdir().unwrap();
    let real_key = stage_real_mod(tmp.path());
    let type_key = synthetic_key("TYPE_ABSOLUTE");
    let coverage = coverage_with(&[real_key, type_key]);
    let nodeid = "test_real_mod.py::test_marker";
    assert!(covered_file_digests(tmp.path(), nodeid, &coverage).is_some());
    fs::remove_file(tmp.path().join("real_mod.py")).unwrap();
    assert!(covered_file_digests(tmp.path(), nodeid, &coverage).is_none());
}

#[test]
fn synthetic_only_coverage_is_reusable_with_empty_digest_map() {
    let tmp = tempfile::tempdir().unwrap();
    let coverage = coverage_with(&[synthetic_key("TYPE_ABSOLUTE"), synthetic_key("FROZEN")]);
    let digests = covered_file_digests(tmp.path(), "x.py::t", &coverage).unwrap();
    assert!(digests.is_empty());
    let entry = cache::RslipCacheEntry::from_outcome(&passed_outcome("x.py::t", coverage), tmp.path());
    assert!(entry_is_reusable(&entry, tmp.path()));
}

#[test]
fn old_empty_digest_entry_misses_once_then_hits_after_rewrite() {
    let tmp = tempfile::tempdir().unwrap();
    let real_key = stage_real_mod(tmp.path());
    let type_key = synthetic_key("TYPE_ABSOLUTE");
    let coverage = coverage_with(&[real_key, type_key]);
    let nodeid = "test_real_mod.py::test_marker";
    let mut stale =
        cache::RslipCacheEntry::from_outcome(&passed_outcome(nodeid, coverage.clone()), tmp.path());
    stale.covered_digests = BTreeMap::new();
    assert!(!entry_is_reusable(&stale, tmp.path()));

    let mut req = rslip_sample_request(tmp.path());
    req.nodeid = nodeid.to_string();
    let fingerprint = rslip_cache_fingerprint(&req).unwrap();
    store_rslip_cache_entry(&req.cache_root, &fingerprint, &stale).unwrap();
    assert!(load_reusable_rslip_cache_entry(&req.cache_root, &fingerprint, tmp.path()).is_none());

    let rewritten =
        cache::RslipCacheEntry::from_outcome(&passed_outcome(nodeid, coverage), tmp.path());
    store_rslip_cache_entry(&req.cache_root, &fingerprint, &rewritten).unwrap();
    assert!(load_reusable_rslip_cache_entry(&req.cache_root, &fingerprint, tmp.path()).is_some());
}

#[test]
fn run_or_reuse_rewrites_old_empty_digests_then_hits() {
    let tmp = tempfile::tempdir().unwrap();
    let real_key = stage_real_mod(tmp.path());
    let type_key = synthetic_key("TYPE_ABSOLUTE");
    let nodeid = "test_real_mod.py::test_marker";
    let coverage = coverage_with(&[real_key.clone(), type_key.clone()]);
    let mut stale =
        cache::RslipCacheEntry::from_outcome(&passed_outcome(nodeid, coverage), tmp.path());
    stale.covered_digests = BTreeMap::new();

    let mut req = rslip_sample_request(tmp.path());
    req.nodeid = nodeid.to_string();
    let fingerprint = rslip_cache_fingerprint(&req).unwrap();
    store_rslip_cache_entry(&req.cache_root, &fingerprint, &stale).unwrap();

    let calls = Rc::new(Cell::new(0));
    let rslip = Rslip::new(mixed_runner(Rc::clone(&calls), real_key, type_key));
    let first = rslip.run_or_reuse(req.clone()).unwrap();
    let second = rslip.run_or_reuse(req.clone()).unwrap();
    let rewritten = load_reusable_rslip_cache_entry(&req.cache_root, &fingerprint, tmp.path());

    assert_eq!(first.cache_status, CacheStatus::MissStored);
    assert_eq!(second.cache_status, CacheStatus::Hit);
    assert_eq!(calls.get(), 1);
    assert!(rewritten.is_some_and(|entry| !entry.covered_digests.is_empty()));
}
