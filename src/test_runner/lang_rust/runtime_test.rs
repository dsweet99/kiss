use super::RustRuntime;
use crate::test_runner::lang_iface::KernelRules;
use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, LanguageRuntime, WitnessStatus,
};
use crate::test_runner::lang_rust::RustKernelRules;
use crate::test_runner::test_selection::SupportedLanguage;
use std::collections::BTreeMap;

#[test]
fn rust_runtime_language() {
    assert_eq!(RustRuntime::default().language(), kiss::Language::Rust);
}

#[test]
fn accepted_summary_emits_cached_passes() {
    let rt = RustRuntime::default();
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
            rust: vec!["a".into()],
        },
    };
    let witness = ExecutionWitness {
        language: kiss::Language::Rust,
        identity_digest: "id".into(),
        selectors: vec!["a".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(1)],
        complete: true,
        generation_id: "g".into(),
        raw_statuses: Vec::new(),
    };
    let summary =
        KernelRules::accepted_summary(&RustKernelRules, &req, &["a".into()], &witness).unwrap();
    assert_eq!(summary.total, 1);
    assert_eq!(summary.cache_hits, 1);
    let _ = rt.list(&req);
}

#[test]
fn rust_runtime_empty_run_and_load_miss() {
    let rt = RustRuntime::default();
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
    let batch = super::runtime::run_rust_selectors(&req, &[], &mut |_| {}).expect("empty run");
    assert_eq!(batch.summary.total, 0);
    if let Ok(listing) = rt.list(&req) {
        assert!(crate::test_runner::ensure_runtime::stored_rows(&req, &rt, &listing).is_none());
    }
    let _ = rt.list(&req).map(|listing| listing.identity);
}

#[test]
fn rust_runtime_nonempty_miss_hits_mode_branches() {
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
        assert!(super::runtime::run_rust_selectors(&req, &["a".into()], &mut |_| {}).is_err());
    }
}

fn demo_crate() -> tempfile::TempDir {
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
    super::nextest::generate_lockfile(tmp.path());
    tmp
}

fn store_rust_record(
    root: &std::path::Path,
    test_id: &str,
    identity: &str,
    status: kiss::rpytest_runner::TestStatus,
) {
    let record = kiss::test_records::TestRecord {
        schema: kiss::test_records::RECORD_SCHEMA.to_string(),
        language: "rust".to_string(),
        test_id: test_id.into(),
        identity: identity.into(),
        deps: BTreeMap::new(),
        status,
        exit_code: Some(0),
        duration: std::time::Duration::from_millis(1),
    };
    kiss::test_records::store_record(&kiss::test_records::records_dir(root, "rust"), &record)
        .unwrap();
}

#[test]
fn records_witness_drops_removed_tests_and_other_identities() {
    use kiss::rpytest_runner::TestStatus;
    let tmp = demo_crate();
    super::nextest::store_records(
        tmp.path(),
        &[
            ("tests::case", TestStatus::Passed),
            ("tests::gone", TestStatus::Passed),
        ],
    );
    store_rust_record(
        tmp.path(),
        "tests::older",
        "older-toolchain",
        TestStatus::Passed,
    );
    crate::test_runner::workspace_selector_cache::store_workspace_selectors(
        tmp.path(),
        &[],
        &[],
        &["tests::case".into(), "tests::older".into()],
        &[],
    )
    .unwrap();
    let witness = super::try_load_rust_execution_witness(tmp.path(), &[]).unwrap();
    assert_eq!(witness.selectors, vec!["tests::case".to_string()]);
    assert_eq!(witness.statuses, vec![WitnessStatus::Passed]);
    assert!(witness.complete);
}

#[test]
fn selectors_for_time_gate_fails_closed_without_report_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let req = EnsureRequest {
        repo_root: tmp.path().to_path_buf(),
        mode: AcceptMode::Subset,
        lang_filter: Some(kiss::Language::Rust),
        ignore: vec![],
        force: false,
        force_selectors: Vec::new(),
        jobs: 1,
        gate: kiss::GateConfig {
            max_unit_test_seconds: vec![("tests/".into(), 2.0)],
            ..kiss::GateConfig::default()
        },
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec!["tests::case".into()],
        },
    };
    let err = RustKernelRules
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
        gate: kiss::GateConfig {
            max_unit_test_seconds: vec![("src/".into(), 2.0)],
            ..kiss::GateConfig::default()
        },
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec!["tests::case".into()],
        },
    };
    RustKernelRules.selectors_for_time_gate(&req, &["tests::case".into()])
}

#[test]
fn live_misses_are_the_tests_whose_records_do_not_hold() {
    use kiss::rpytest_runner::TestStatus;
    let tmp = demo_crate();
    super::nextest::store_records(
        tmp.path(),
        &[("pass", TestStatus::Passed), ("fail", TestStatus::Failed)],
    );
    let mut req = EnsureRequest {
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
            rust: vec!["pass".into(), "fail".into(), "extra".into()],
        },
    };
    let rt = RustRuntime::default();
    let listing = rt.list(&req).expect("listing");
    let witness = crate::test_runner::ensure_runtime::stored_rows(&req, &rt, &listing)
        .expect("records witness")
        .witness;
    assert_eq!(
        witness.selectors,
        vec!["fail".to_string(), "pass".to_string()]
    );
    assert_eq!(
        witness.statuses,
        vec![WitnessStatus::Failed, WitnessStatus::Passed]
    );
    assert!(!witness.complete);
    let planned = req.planned.rust.clone();
    assert_eq!(
        RustKernelRules.live_misses(&req, &planned, "", Some(&witness)),
        planned
    );
    req.force = true;
    assert_eq!(
        RustKernelRules.live_misses(&req, &planned, "", Some(&witness)),
        planned
    );
}

#[test]
fn records_with_stale_inputs_do_not_hold() {
    use kiss::rpytest_runner::TestStatus;
    let tmp = demo_crate();
    super::nextest::store_records(tmp.path(), &[("tests::case", TestStatus::Passed)]);
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn case() { assert!(true); }\n}\n",
    )
    .unwrap();
    let witness = super::try_load_rust_execution_witness(tmp.path(), &[]);
    assert!(
        witness.is_err() || witness.unwrap().selectors.is_empty(),
        "an edited Rust source must invalidate every Rust record"
    );
}
