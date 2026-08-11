//! Smoke coverage for RustRuntime trait methods.

use super::RustRuntime;
use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, LanguageRuntime, PublishBatch, WitnessScope,
    WitnessStatus,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[test]
fn rust_runtime_language_and_dry_run_and_indexable() {
    let rt = RustRuntime;
    assert_eq!(rt.language(), kiss::Language::Rust);
    let lines = rt
        .dry_run_lines(&["a".into()], true, &[], 1)
        .expect("dry run");
    assert!(lines.iter().any(|l| l.contains("RUST")));
    let root = PathBuf::from(".");
    let _ = rt.is_indexable_source(Path::new("src/lib.rs"), &root);
}

#[test]
fn accepted_summary_emits_cached_passes() {
    let rt = RustRuntime;
    let req = EnsureRequest {
        repo_root: PathBuf::from("."),
        mode: AcceptMode::Subset,
        lang_filter: Some(kiss::Language::Rust),
        ignore: vec![],
        force: false,
        jobs: 1,
        python_extra: vec![],
        rust_extra: vec![],
        planned_python: vec![],
        planned_rust: vec!["a".into()],
    };
    let witness = ExecutionWitness {
        language: "rust".into(),
        scope: WitnessScope::Full,
        identity_digest: "id".into(),
        selectors: vec!["a".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![1],
        covered_lines: BTreeMap::new(),
        complete: true,
        generation_id: "g".into(),
    };
    let summary = rt.accepted_summary(&req, &["a".into()], &witness);
    assert_eq!(summary.total, 1);
    assert_eq!(summary.cache_hits, 1);
    let tmp = tempfile::tempdir().unwrap();
    let _ = rt.discover_universe(&req);
    let _ = rt.coverage_snapshot(tmp.path());
    let _ = rt.status_timing_snapshot(tmp.path());
}

#[test]
fn rust_runtime_empty_run_and_load_miss() {
    let rt = RustRuntime;
    let tmp = tempfile::tempdir().unwrap();
    let req = EnsureRequest {
        repo_root: tmp.path().to_path_buf(),
        mode: AcceptMode::All,
        lang_filter: Some(kiss::Language::Rust),
        ignore: vec![],
        force: false,
        jobs: 1,
        python_extra: vec![],
        rust_extra: vec![],
        planned_python: vec![],
        planned_rust: vec!["a".into()],
    };
    let batch = rt.run_selectors(&req, &[]).expect("empty run");
    assert_eq!(batch.summary.total, 0);
    assert!(rt.load_full_witness(tmp.path()).is_err());
    let _ = rt.current_identity(&req);
    let publish = PublishBatch {
        selectors: vec!["a".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![1],
        covered_lines: BTreeMap::new(),
        publication_universe: Some(vec!["a".into()]),
        summary: Default::default(),
    };
    let _ = rt.publish_outcomes(&req, &publish);
}

#[test]
fn rust_runtime_nonempty_miss_hits_mode_branches() {
    let rt = RustRuntime;
    let tmp = tempfile::tempdir().unwrap();
    for mode in [AcceptMode::All, AcceptMode::Subset] {
        let req = EnsureRequest {
            repo_root: tmp.path().to_path_buf(),
            mode,
            lang_filter: Some(kiss::Language::Rust),
            ignore: vec![],
            force: false,
            jobs: 1,
            python_extra: vec![],
            rust_extra: vec![],
            planned_python: vec![],
            planned_rust: vec!["a".into(), "b".into()],
        };
        assert!(rt.run_selectors(&req, &["a".into()]).is_err());
    }
}
