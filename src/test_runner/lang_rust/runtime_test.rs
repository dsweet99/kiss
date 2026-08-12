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
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed { python: vec![], rust: vec![] },
        planned: crate::test_runner::language_keyed::LanguageKeyed { python: vec![], rust: vec!["a".into()] },
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
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed { python: vec![], rust: vec![] },
        planned: crate::test_runner::language_keyed::LanguageKeyed { python: vec![], rust: vec!["a".into()] },
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
            gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed { python: vec![], rust: vec![] },
        planned: crate::test_runner::language_keyed::LanguageKeyed { python: vec![], rust: vec!["a".into(), "b".into()] },
        };
        assert!(rt.run_selectors(&req, &["a".into()]).is_err());
    }
}

#[test]
fn selectors_for_time_gate_uses_typed_logical_to_report_boundary() {
    // Empty repo → empty map → unmapped logicals stay explicit (same as report_id_for_logical).
    let rt = RustRuntime;
    let tmp = tempfile::tempdir().unwrap();
    let req = EnsureRequest {
        repo_root: tmp.path().to_path_buf(),
        mode: AcceptMode::Subset,
        lang_filter: Some(kiss::Language::Rust),
        ignore: vec![],
        force: false,
        jobs: 1,
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec!["tests::case".into()],
        },
    };
    let out = rt.selectors_for_time_gate(&req, &["tests::case".into(), "bare".into()]);
    assert_eq!(out, vec!["tests::case".to_string(), "bare".to_string()]);
    // Matches the shared typed helper (regression: bypassing LogicalSelectorId would drift).
    let map = crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
        &req.repo_root,
        &req.ignore,
    );
    assert_eq!(
        out,
        crate::test_runner::selector_ids::report_strings_for_logical_strings(
            &map,
            &["tests::case".into(), "bare".into()],
        )
    );
}
