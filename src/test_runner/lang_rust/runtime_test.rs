use super::RustRuntime;
use crate::test_runner::lang_iface::KernelRules;
use crate::test_runner::lang_iface::{AcceptMode, EnsureRequest, LanguageRuntime, WitnessStatus};
use crate::test_runner::lang_rust::RustKernelRules;
use crate::test_runner::test_selection::SupportedLanguage;
use std::collections::BTreeMap;

#[test]
fn rust_runtime_language() {
    assert_eq!(RustRuntime.language(), kiss::Language::Rust);
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
