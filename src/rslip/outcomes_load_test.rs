use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::time::Duration;

use crate::rpytest_runner::TestStatus;
use crate::rslip::cache::{RslipCacheEntry, rslip_cache_fingerprint, store_rslip_cache_entry};
use crate::rslip::{
    CacheStatus, LineCoverage, RslipOutcome, load_cached_outcomes_many_trusting_population,
};

use super::rslip_sample_request;

fn outcome_with_lines(nodeid: &str, file: &str, lines: &[u32]) -> RslipOutcome {
    RslipOutcome {
        nodeid: nodeid.to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        coverage: LineCoverage {
            files: BTreeMap::from([(file.to_string(), lines.iter().copied().collect())]),
        },
        cache_status: CacheStatus::MissStored,
        stdout: None,
        stderr: None,
    }
}

#[test]
fn trusting_load_does_not_union_coverage_from_other_fingerprints() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("app.py"), "a\nb\nc\n").unwrap();
    fs::write(
        root.join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(root);
    fs::create_dir_all(req.cache_root.join("entries")).unwrap();
    let current_fp = rslip_cache_fingerprint(&req).unwrap();
    let thin =
        RslipCacheEntry::from_outcome(&outcome_with_lines(&req.nodeid, "app.py", &[3]), root);
    let fat =
        RslipCacheEntry::from_outcome(&outcome_with_lines(&req.nodeid, "app.py", &[1, 2, 3]), root);
    store_rslip_cache_entry(&req.cache_root, &current_fp, &thin).unwrap();
    store_rslip_cache_entry(&req.cache_root, "other-fingerprint", &fat).unwrap();
    let loaded = load_cached_outcomes_many_trusting_population(&[req]);
    let outcome = loaded[0].as_ref().unwrap().as_ref().unwrap();
    assert_eq!(
        outcome.coverage.files.get("app.py"),
        Some(&BTreeSet::from([3]))
    );
}

#[test]
fn trusting_load_skips_sibling_files_with_stale_digests() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("app.py"), "old\n").unwrap();
    fs::write(
        root.join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(root);
    fs::create_dir_all(req.cache_root.join("entries")).unwrap();
    let stale =
        RslipCacheEntry::from_outcome(&outcome_with_lines(&req.nodeid, "app.py", &[1, 2, 9]), root);
    fs::write(root.join("app.py"), "new\n").unwrap();
    let current_fp = rslip_cache_fingerprint(&req).unwrap();
    let thin =
        RslipCacheEntry::from_outcome(&outcome_with_lines(&req.nodeid, "app.py", &[1]), root);
    store_rslip_cache_entry(&req.cache_root, "stale-fingerprint", &stale).unwrap();
    store_rslip_cache_entry(&req.cache_root, &current_fp, &thin).unwrap();
    let loaded = load_cached_outcomes_many_trusting_population(&[req]);
    let outcome = loaded[0].as_ref().unwrap().as_ref().unwrap();
    assert_eq!(
        outcome.coverage.files.get("app.py"),
        Some(&BTreeSet::from([1]))
    );
}
