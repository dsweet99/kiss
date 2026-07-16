#[test]
fn refresh_guard_env_name_is_stable() {
    assert_eq!(
        super::CHECK_RUNTIME_REFRESH_ACTIVE_ENV,
        "KISS_CHECK_RUNTIME_REFRESH_ACTIVE"
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
) -> rust_llvm_cov_runner::ValidatedCheckAggregate {
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
) -> rust_llvm_cov_runner::ValidatedCheckAggregate {
    let selector_binary_ids = selector_maps
        .iter()
        .map(|(selector, ids)| {
            (
                (*selector).to_string(),
                ids.iter().map(|id| (*id).to_string()).collect::<Vec<_>>(),
            )
        })
        .collect();
    rust_llvm_cov_runner::ValidatedCheckAggregate {
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
                    rust_llvm_cov_runner::CheckAggregateBinaryRecord {
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

fn test_binary(id: &str, digest: &str) -> rust_llvm_cov_runner::RustTestBinaryIdentity {
    rust_llvm_cov_runner::RustTestBinaryIdentity {
        id: id.to_string(),
        executable: format!("target/{id}"),
        digest: digest.to_string(),
    }
}
