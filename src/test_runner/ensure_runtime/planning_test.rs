//! Planning API unit coverage.

use super::{ensure_request_for_selectors, ensure_request_from_planned};
use crate::test_runner::lang_iface::AcceptMode;
use crate::test_runner::PlannedSelectors;
use std::path::PathBuf;

#[test]
fn ensure_request_from_planned_copies_selectors_and_root() {
    let planned = PlannedSelectors {
        repo_root: PathBuf::from("/repo"),
        sel: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec!["a".into()],
            rust: vec!["b".into()],
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: vec![],
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        coverage_decision_engine_used: false,
        selection_basis: crate::test_runner::language_keyed::LanguageKeyed {
            python: crate::test_runner::coverage_decision::SelectionBasis::Current,
            rust: crate::test_runner::coverage_decision::SelectionBasis::Current,
        },
        ignore: vec!["tmp".into()],
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    };
    let req = ensure_request_from_planned(super::EnsureFromPlanned {
        planned: &planned,
        mode: AcceptMode::Subset,
        lang_filter: Some(kiss::Language::Python),
        force: true,
        jobs: 4,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: &["-p".into()],
            rust: &["--extra".into()],
        },
        repo_root_override: None,
        gate: kiss::GateConfig::default(),
    });
    assert_eq!(req.planned.python, vec!["a".to_string()]);
    assert_eq!(req.planned.rust, vec!["b".to_string()]);
    assert_eq!(req.repo_root, PathBuf::from("/repo"));
    assert!(req.force);
    assert_eq!(req.jobs, 4);
}

#[test]
fn ensure_request_carries_session_gate_without_reload() {
    let planned = PlannedSelectors {
        repo_root: PathBuf::from("/repo"),
        sel: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec!["a".into()],
            rust: vec![],
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: vec![],
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        coverage_decision_engine_used: false,
        selection_basis: crate::test_runner::language_keyed::LanguageKeyed {
            python: crate::test_runner::coverage_decision::SelectionBasis::Current,
            rust: crate::test_runner::coverage_decision::SelectionBasis::Current,
        },
        ignore: vec![],
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    };
    let session_gate = kiss::GateConfig {
        max_unit_test_seconds: vec![("session-only".into(), 7.0), ("*".into(), 1.0)],
        ..kiss::GateConfig::default()
    };
    let req = ensure_request_from_planned(super::EnsureFromPlanned {
        planned: &planned,
        mode: AcceptMode::Subset,
        lang_filter: Some(kiss::Language::Python),
        force: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        repo_root_override: None,
        gate: session_gate.clone(),
    });
    assert_eq!(
        req.gate.max_unit_test_seconds, session_gate.max_unit_test_seconds,
        "EnsureRequest must keep the CLI session gate, not reload from cwd"
    );
    assert!(
        (kiss::limit_for_selector(&req.gate.max_unit_test_seconds, "session-only/x.py::t") - 7.0)
            .abs()
            < f64::EPSILON
    );
}

#[test]
fn ensure_request_for_selectors_sets_lang_filter() {
    let req = ensure_request_for_selectors(
        PathBuf::from("/r").as_path(),
        &[],
        1,
        kiss::Language::Rust,
        false,
        vec![],
        vec!["t".into()],
        kiss::GateConfig::default(),
    );
    assert_eq!(req.lang_filter, Some(kiss::Language::Rust));
    assert_eq!(req.planned.rust, vec!["t".to_string()]);
}
