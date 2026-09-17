use std::collections::BTreeMap;
use std::path::Path;

use super::{
    clear_published_witness_memo_for_tests, file_stamp, memo_witness, stash_published_witness,
    try_recall_published_rust_covered_lines,
};
use crate::test_runner::lang_iface::{ExecutionWitness, WitnessScope, WitnessStatus};

fn memo_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn sample_witness(complete: bool, generation_id: &str) -> ExecutionWitness {
    ExecutionWitness {
        language: "rust".into(),
        scope: WitnessScope::Full,
        identity_digest: "id".into(),
        selectors: vec!["a".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(1)],
        covered_lines: BTreeMap::from([("f.rs".into(), vec![3])]),
        complete,
        generation_id: generation_id.into(),
        raw_statuses: vec![WitnessStatus::Passed],
    }
}

fn write_stamp_file(dir: &Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, "body").unwrap();
    path
}

#[test]
fn file_stamp_rejects_empty_and_records_nonempty() {
    let tmp = tempfile::tempdir().unwrap();
    let empty = tmp.path().join("empty");
    std::fs::write(&empty, "").unwrap();
    assert!(file_stamp(&empty).is_none());
    assert!(file_stamp(&tmp.path().join("missing")).is_none());
    let body = write_stamp_file(tmp.path(), "body");
    assert!(file_stamp(&body).is_some());
}

#[test]
fn memo_hits_matching_stamp_and_misses_other_repo() {
    let _guard = memo_test_guard();
    clear_published_witness_memo_for_tests();
    let home = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let path = write_stamp_file(home.path(), "w.json");
    stash_published_witness(home.path(), &path, sample_witness(true, "g"));
    assert!(memo_witness(home.path(), &path).is_some());
    assert!(memo_witness(other.path(), &path).is_none());
}

#[test]
fn stash_uses_generation_stamp_when_file_missing() {
    let _guard = memo_test_guard();
    clear_published_witness_memo_for_tests();
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing.json");
    stash_published_witness(tmp.path(), &missing, sample_witness(true, "gid"));
    assert!(memo_witness(tmp.path(), &missing).is_some());
}

#[test]
fn try_recall_returns_covered_lines_when_complete() {
    let _guard = memo_test_guard();
    clear_published_witness_memo_for_tests();
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    std::fs::create_dir_all(&cache).unwrap();
    let path = cache.join("execution_witness.json");
    std::fs::write(&path, "x").unwrap();
    stash_published_witness(tmp.path(), &path, sample_witness(true, "g"));
    let (generation, lines) = try_recall_published_rust_covered_lines(tmp.path()).unwrap();
    assert_eq!(generation, "g");
    assert_eq!(lines.get("f.rs").map(|s| s.len()), Some(1));
}

#[test]
fn try_recall_skips_incomplete_witness() {
    let _guard = memo_test_guard();
    clear_published_witness_memo_for_tests();
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    std::fs::create_dir_all(&cache).unwrap();
    let path = cache.join("execution_witness.json");
    std::fs::write(&path, "x").unwrap();
    stash_published_witness(tmp.path(), &path, sample_witness(false, "g"));
    assert!(try_recall_published_rust_covered_lines(tmp.path()).is_none());
}
