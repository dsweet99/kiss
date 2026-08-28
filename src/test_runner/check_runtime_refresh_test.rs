#[test]
fn ensure_rust_runtime_coverage_shared_is_named() {
    let _ = super::ensure_rust_runtime_coverage_shared;
}

#[test]
fn refresh_guard_env_name_is_stable() {
    assert_eq!(
        super::COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV,
        "KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE"
    );
}

#[test]
fn rust_publication_error_mentions_refresh_phase() {
    let err = super::CoverageRefreshError::Publication {
        language: "Rust",
        reason: "aggregate export failed".to_string(),
    };
    let rendered = err.to_string();
    assert!(rendered.contains("failed to refresh Rust runtime line coverage"));
    assert!(rendered.contains("during publication"));
}

#[test]
fn aggregate_repair_identity_only_retains_current_maps() {
    let selectors = vec!["pkg::bin$alpha".to_string()];
    let prior = aggregate_prior(&selectors, &[("bin-a", "digest-a")]);
    let current_maps =
        std::collections::BTreeMap::from([(selectors[0].clone(), vec!["bin-a".to_string()])]);
    let current_binaries = vec![test_binary("bin-a", "digest-a")];

    let decision = super::classify_check_aggregate_repair(
        &selectors,
        &prior,
        &current_maps,
        &current_binaries,
    );

    assert!(matches!(
        decision,
        super::CheckAggregateRepairDecision::IdentityOnly { .. }
    ));
}

#[test]
fn aggregate_repair_reruns_selectors_for_changed_binary() {
    let selectors = vec!["pkg::bin$alpha".to_string(), "pkg::bin$beta".to_string()];
    let prior = aggregate_prior_with_maps(
        &selectors,
        &[("bin-a", "old"), ("bin-b", "stable")],
        &[
            (selectors[0].as_str(), vec!["bin-a"]),
            (selectors[1].as_str(), vec!["bin-b"]),
        ],
    );
    let current_maps = std::collections::BTreeMap::from([
        (selectors[0].clone(), vec!["bin-a".to_string()]),
        (selectors[1].clone(), vec!["bin-b".to_string()]),
    ]);
    let current_binaries = vec![test_binary("bin-a", "new"), test_binary("bin-b", "stable")];

    let decision = super::classify_check_aggregate_repair(
        &selectors,
        &prior,
        &current_maps,
        &current_binaries,
    );

    match decision {
        super::CheckAggregateRepairDecision::Rerun {
            prior_generation: _,
            rerun_selectors,
            replacement_binary_ids,
            retained_binary_line_maps,
        } => {
            assert_eq!(rerun_selectors, vec![selectors[0].clone()]);
            assert_eq!(
                replacement_binary_ids,
                std::collections::BTreeSet::from(["bin-a".to_string()])
            );
            assert!(retained_binary_line_maps.contains_key("bin-b"));
            assert!(!retained_binary_line_maps.contains_key("bin-a"));
        }
        other => panic!("expected rerun repair, got {other:?}"),
    }
}

#[test]
fn aggregate_repair_full_refresh_when_mapping_missing() {
    let selectors = vec!["pkg::bin$alpha".to_string()];
    let prior = aggregate_prior(&selectors, &[("bin-a", "digest-a")]);
    let current_binaries = vec![test_binary("bin-a", "digest-a")];

    let decision = super::classify_check_aggregate_repair(
        &selectors,
        &prior,
        &std::collections::BTreeMap::new(),
        &current_binaries,
    );

    assert_eq!(decision, super::CheckAggregateRepairDecision::FullRefresh);
}

#[test]
fn aggregate_repair_changed_multibinary_mapping_replaces_current_binary() {
    let selectors = vec!["shared_name".to_string()];
    let prior = aggregate_prior(&selectors, &[("bin-a", "digest-a")]);
    let current_maps = std::collections::BTreeMap::from([(
        selectors[0].clone(),
        vec!["bin-a".to_string(), "bin-b".to_string()],
    )]);
    let current_binaries = vec![
        test_binary("bin-a", "digest-a"),
        test_binary("bin-b", "digest-b"),
    ];

    let decision = super::classify_check_aggregate_repair(
        &selectors,
        &prior,
        &current_maps,
        &current_binaries,
    );

    match decision {
        super::CheckAggregateRepairDecision::Rerun {
            replacement_binary_ids,
            ..
        } => {
            assert_eq!(
                replacement_binary_ids,
                std::collections::BTreeSet::from(["bin-a".to_string(), "bin-b".to_string()])
            );
        }
        other => panic!("expected rerun repair, got {other:?}"),
    }
}

#[test]
fn aggregate_repair_ignores_unmapped_binary_digest_changes() {
    let selectors = vec!["pkg::bin$alpha".to_string()];
    let prior = aggregate_prior(&selectors, &[("bin-a", "digest-a")]);
    let current_maps =
        std::collections::BTreeMap::from([(selectors[0].clone(), vec!["bin-a".to_string()])]);
    let current_binaries = vec![
        test_binary("bin-a", "digest-a"),
        test_binary("unmapped", "new-digest"),
    ];

    let decision = super::classify_check_aggregate_repair(
        &selectors,
        &prior,
        &current_maps,
        &current_binaries,
    );

    assert!(matches!(
        decision,
        super::CheckAggregateRepairDecision::IdentityOnly { .. }
    ));
}

fn aggregate_prior(
    selectors: &[String],
    binaries: &[(&str, &str)],
) -> kiss::rust_llvm_cov_runner::ValidatedCheckAggregate {
    let selector_maps = selectors
        .iter()
        .map(|selector| {
            let ids = binaries.iter().map(|(id, _)| *id).collect::<Vec<_>>();
            (selector.as_str(), ids)
        })
        .collect::<Vec<_>>();
    aggregate_prior_with_maps(selectors, binaries, &selector_maps)
}

fn aggregate_prior_with_maps(
    selectors: &[String],
    binaries: &[(&str, &str)],
    selector_maps: &[(&str, Vec<&str>)],
) -> kiss::rust_llvm_cov_runner::ValidatedCheckAggregate {
    let selector_binary_ids = selector_maps
        .iter()
        .map(|(selector, ids)| {
            (
                (*selector).to_string(),
                ids.iter().map(|id| (*id).to_string()).collect::<Vec<_>>(),
            )
        })
        .collect();
    kiss::rust_llvm_cov_runner::ValidatedCheckAggregate {
        input_fingerprint: "old-input".to_string(),
        generation_fingerprint: "old-generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        ordinary_source_digests: Default::default(),
        selectors: selectors.to_vec(),
        selector_binary_ids,
        binaries: binaries
            .iter()
            .map(|(id, digest)| {
                (
                    (*id).to_string(),
                    kiss::rust_llvm_cov_runner::CheckAggregateBinaryRecord {
                        id: (*id).to_string(),
                        executable: format!("target/{id}"),
                        digest: (*digest).to_string(),
                        line_map: std::collections::BTreeMap::from([(
                            "src/lib.rs".to_string(),
                            std::collections::BTreeSet::from([1]),
                        )]),
                    },
                )
            })
            .collect(),
        aggregate_covered_lines: std::collections::BTreeMap::from([(
            "src/lib.rs".to_string(),
            std::collections::BTreeSet::from([1]),
        )]),
        integrity_fingerprint: "unused-by-classifier".to_string(),
    }
}

fn test_binary(id: &str, digest: &str) -> kiss::rust_llvm_cov_runner::RustTestBinaryIdentity {
    kiss::rust_llvm_cov_runner::RustTestBinaryIdentity {
        id: id.to_string(),
        executable: format!("target/{id}"),
        digest: digest.to_string(),
    }
}

#[path = "check_runtime_refresh_witness_test.rs"]
mod witness_tests;

#[test]
fn coverage_refresh_error_constructors_and_display_cover_all_arms() {
    let lock = super::CoverageRefreshError::lock("Rust", "busy");
    assert!(lock.to_string().contains("lock acquisition"));
    let discovery = super::CoverageRefreshError::discovery("Python", "parse failed");
    assert!(discovery.to_string().contains("test discovery"));
    let publication = super::CoverageRefreshError::publication("Rust", "write failed");
    assert!(publication.to_string().contains("publication"));
    let validation = super::CoverageRefreshError::PostRefreshValidation {
        language: "Rust",
        reason: "missing aggregate".into(),
    };
    assert!(validation.to_string().contains("post-refresh validation"));
    let _via_ctor = super::CoverageRefreshError::validation(
        "Rust",
        crate::test_runner::check_line_coverage::RuntimeCoverageLoadError {
            language: "Rust",
            reason: "missing aggregate".into(),
            problem_selectors: Vec::new(),
        },
    );
    let exec = super::CoverageRefreshError::TestExecution {
        language: "Rust",
        total: 3,
        failed: 1,
        exit_code: 1,
    };
    assert!(exec.to_string().contains("1/3"));
}

#[test]
fn finalize_population_summary_labeled_uses_caller_label_not_kiss_cov() {
    use crate::test_runner::runners::SelectorExecutionSummary;

    let tmp = tempfile::tempdir().unwrap();
    let summary = SelectorExecutionSummary {
        exit_code: 1,
        total: 2,
        failed: 1,
        ..Default::default()
    };

    let err_cov = super::finalize_population_summary(tmp.path(), &[], &summary, false).unwrap_err();
    let err_test =
        super::finalize_population_summary_labeled(tmp.path(), &[], &summary, false, "kiss test")
            .unwrap_err();

    assert!(matches!(
        err_cov,
        super::CoverageRefreshError::TestExecution { .. }
    ));
    assert!(matches!(
        err_test,
        super::CoverageRefreshError::TestExecution { .. }
    ));

    assert_eq!(err_cov.to_string(), err_test.to_string());
}

#[test]
fn ensure_check_runtime_coverage_no_languages_is_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let required = crate::test_runner::check_line_coverage::RequiredCoverageLanguages {
        python: false,
        rust: false,
    };
    super::ensure_check_runtime_coverage(
        tmp.path(),
        required,
        &[],
        1,
        &[],
        &kiss::GateConfig::default(),
    )
    .unwrap();
}

#[test]
fn coverage_refresh_stats_for_rust_helper_sets_rust_slot() {
    let stats = super::CoverageRefreshStats::for_rust(super::LanguageRefreshStats {
        test_instances: 3,
        full_refresh: true,
        ..Default::default()
    });
    assert_eq!(stats.by_language.rust.test_instances, 3);
    assert!(stats.by_language.rust.full_refresh);
    assert_eq!(stats.by_language.python.test_instances, 0);
}

#[test]
fn runtime_refresh_trait_language_identity() {
    assert_eq!(
        super::CoverageRuntimeRefresh::language(&super::PythonRuntimeRefresh),
        kiss::Language::Python
    );
    assert_eq!(
        super::CoverageRuntimeRefresh::language(&super::RustRuntimeRefresh),
        kiss::Language::Rust
    );
}
