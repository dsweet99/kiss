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
        force_selectors: Vec::new(),
        jobs: 1,
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec!["a".into()],
        },
    };
    let witness = ExecutionWitness {
        language: "rust".into(),
        scope: WitnessScope::Full,
        identity_digest: "id".into(),
        selectors: vec!["a".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(1)],
        covered_lines: BTreeMap::new(),
        complete: true,
        generation_id: "g".into(),
    };
    let summary = rt.accepted_summary(&req, &["a".into()], &witness);
    assert_eq!(summary.total, 1);
    assert_eq!(summary.cache_hits, 1);
    assert!(!summary.rust_derived_repair);
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
        force_selectors: Vec::new(),
        jobs: 1,
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec!["a".into()],
        },
    };
    let batch = rt.run_selectors(&req, &[]).expect("empty run");
    assert_eq!(batch.summary.total, 0);
    assert!(rt.load_full_witness(tmp.path()).is_err());
    let _ = rt.current_identity(&req);
    let publish = PublishBatch {
        selectors: vec!["a".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(1)],
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
            force_selectors: Vec::new(),
            jobs: 1,
            gate: kiss::GateConfig::default(),
            extras: crate::test_runner::language_keyed::LanguageKeyed {
                python: vec![],
                rust: vec![],
            },
            planned: crate::test_runner::language_keyed::LanguageKeyed {
                python: vec![],
                rust: vec!["a".into(), "b".into()],
            },
        };
        assert!(rt.run_selectors(&req, &["a".into()]).is_err());
    }
}

#[test]
fn prune_removed_rust_witness_selectors_drops_stale_entries() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn case() {}\n}\n",
    )
    .unwrap();
    let mut witness = crate::test_runner::lang_iface::ExecutionWitness {
        language: "rust".into(),
        scope: crate::test_runner::lang_iface::WitnessScope::Full,
        identity_digest: "id".into(),
        selectors: vec![
            "tests::case".into(),
            "force_miss_batch_writes_warm_hit_seal_for_later_hit".into(),
        ],
        statuses: vec![
            crate::test_runner::lang_iface::WitnessStatus::Passed,
            crate::test_runner::lang_iface::WitnessStatus::Passed,
        ],
        durations_ns: vec![Some(1), Some(1)],
        covered_lines: Default::default(),
        complete: true,
        generation_id: "gen".into(),
    };
    super::witness_store::prune_removed_rust_witness_selectors(tmp.path(), &mut witness).unwrap();
    assert_eq!(witness.selectors, vec!["tests::case".to_string()]);
    assert_eq!(witness.statuses.len(), 1);
}

#[test]
fn selectors_for_time_gate_fails_closed_without_report_ids() {
    let rt = RustRuntime;
    let tmp = tempfile::tempdir().unwrap();
    let req = EnsureRequest {
        repo_root: tmp.path().to_path_buf(),
        mode: AcceptMode::Subset,
        lang_filter: Some(kiss::Language::Rust),
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
            python: vec![],
            rust: vec!["tests::case".into()],
        },
    };
    let err = rt
        .selectors_for_time_gate(&req, &["tests::case".into(), "bare".into()])
        .unwrap_err();
    assert!(
        err.contains("missing PATH::symbol report id"),
        "unexpected err: {err}"
    );
}

#[test]
fn selectors_for_time_gate_maps_logical_to_path_symbol() {
    assert_eq!(
        time_gate_report_ids(&[]).unwrap(),
        vec!["src/lib.rs::case".to_string()]
    );
}

#[test]
fn selectors_for_time_gate_maps_tests_in_ignored_files() {
    assert_eq!(
        time_gate_report_ids(&["lib.rs".to_string()]).unwrap(),
        vec!["src/lib.rs::case".to_string()]
    );
}

fn time_gate_report_ids(ignore: &[String]) -> Result<Vec<String>, String> {
    let rt = RustRuntime;
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn case() {}\n}\n",
    )
    .unwrap();
    let req = EnsureRequest {
        repo_root: tmp.path().to_path_buf(),
        mode: AcceptMode::Subset,
        lang_filter: Some(kiss::Language::Rust),
        ignore: ignore.to_vec(),
        force: false,
        force_selectors: Vec::new(),
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
    rt.selectors_for_time_gate(&req, &["tests::case".into()])
}
