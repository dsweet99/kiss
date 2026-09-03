use super::PythonRuntime;
use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, LanguageRuntime, PublishBatch, WitnessScope,
    WitnessStatus,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[test]
fn python_runtime_language_dry_run_and_indexable() {
    let rt = PythonRuntime;
    assert_eq!(rt.language(), kiss::Language::Python);
    let lines = rt
        .dry_run_lines(&["t.py::a".into()], true, &[], 1)
        .expect("dry run");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("PYTHON") || l.contains("pytest") || l.contains("t.py"))
    );
    let root = PathBuf::from(".");
    let _ = rt.is_indexable_source(Path::new("app.py"), &root);
}

#[test]
fn python_accepted_summary_counts_hits() {
    let rt = PythonRuntime;
    let req = EnsureRequest {
        repo_root: PathBuf::from("."),
        mode: AcceptMode::All,
        lang_filter: Some(kiss::Language::Python),
        ignore: vec![],
        force: false,
        force_selectors: Vec::new(),
        jobs: 1,
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec!["a".into()],
            rust: vec![],
        },
    };
    let witness = ExecutionWitness {
        language: "python".into(),
        scope: WitnessScope::Full,
        identity_digest: "id".into(),
        selectors: vec!["a".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(1)],
        covered_lines: BTreeMap::new(),
        complete: true,
        generation_id: "g".into(),
        raw_statuses: Vec::new(),
    };
    let summary = rt.accepted_summary(&req, &["a".into()], &witness).unwrap();
    assert_eq!(summary.cache_hits, 1);
    let tmp = tempfile::tempdir().unwrap();
    let _ = rt.discover_universe(&req);
    let _ = rt.coverage_snapshot(tmp.path());
    let _ = rt.status_timing_snapshot(tmp.path());
}

#[test]
fn python_runtime_empty_run_and_identity_paths() {
    let rt = PythonRuntime;
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let req = EnsureRequest {
        repo_root: tmp.path().to_path_buf(),
        mode: AcceptMode::All,
        lang_filter: Some(kiss::Language::Python),
        ignore: vec![],
        force: false,
        force_selectors: Vec::new(),
        jobs: 1,
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec!["a".into()],
            rust: vec![],
        },
    };
    let batch = rt.run_selectors(&req, &[]).expect("empty");
    assert_eq!(batch.summary.total, 0);
    assert!(rt.load_full_witness(tmp.path()).is_err());
    let _ = rt.current_identity(&req);
    let publish = PublishBatch {
        selectors: vec![],
        statuses: vec![],
        durations_ns: vec![],
        covered_lines: BTreeMap::new(),
        publication_universe: Some(vec![]),
        summary: Default::default(),
    };
    let _ = rt.publish_outcomes(&req, &publish);
    let covering = PublishBatch {
        publication_universe: None,
        ..publish
    };
    let _ = rt.publish_outcomes(&req, &covering);
}
