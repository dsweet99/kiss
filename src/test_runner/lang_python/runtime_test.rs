use super::PythonRuntime;
use crate::test_runner::coverage_decision::SupportedLanguage;
use crate::test_runner::lang_iface::KernelRules;
use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, LanguageRuntime, WitnessStatus,
};
use std::collections::BTreeMap;

#[test]
fn python_runtime_language_and_no_generation_publish() {
    let rt = PythonRuntime;
    assert_eq!(rt.language(), kiss::Language::Python);
    let src = include_str!("runtime.rs");
    assert!(!src.contains("python_generation_publish"));
}

#[test]
fn python_accepted_summary_counts_hits() {
    let rt = PythonRuntime;
    let tmp = tempfile::tempdir().unwrap();
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
    let witness = ExecutionWitness {
        language: "python".into(),
        identity_digest: "id".into(),
        selectors: vec!["a".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(1)],
        covered_lines: BTreeMap::new(),
        complete: true,
        generation_id: "g".into(),
        raw_statuses: Vec::new(),
    };
    let summary = KernelRules::accepted_summary(
        &crate::test_runner::lang_python::PythonKernelRules,
        &req,
        &["a".into()],
        &witness,
    )
    .unwrap();
    assert_eq!(summary.cache_hits, 1);
    let _ = rt.list(&req);
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
    let batch = super::runtime::run_python_selectors(&req, &[], &mut |_| {}).expect("empty");
    assert_eq!(batch.summary.total, 0);
    if let Ok(listing) = rt.list(&req) {
        assert!(crate::test_runner::ensure_runtime::stored_rows(&req, &rt, &listing).is_none());
    }
    let _ = rt.list(&req).map(|listing| listing.identity);
}
