use std::collections::{BTreeMap, BTreeSet};

use rust_llvm_cov_runner::{RustLineCoverage, RustReusableSelectorEntry, RustTestBinaryIdentity};

use super::classify_incremental_rust_selectors;

fn prior_state() -> rust_llvm_cov_runner::RustPopulationState {
    rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: "prior-input".to_string(),
        generation_fingerprint: "prior-gen".to_string(),
        selection_context_fingerprint: "ctx".to_string(),
        entries_fingerprint: "entries".to_string(),
        selectors: vec!["alpha".to_string(), "beta".to_string()],
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::from([
            (
                "bin-a".to_string(),
                RustTestBinaryIdentity {
                    id: "bin-a".to_string(),
                    executable: "bin-a".to_string(),
                    digest: "aaaaaaaaaaaaaaaa".to_string(),
                },
            ),
            (
                "bin-b".to_string(),
                RustTestBinaryIdentity {
                    id: "bin-b".to_string(),
                    executable: "bin-b".to_string(),
                    digest: "bbbbbbbbbbbbbbbb".to_string(),
                },
            ),
        ]),
    }
}

fn entry(selector: &str, binary_id: &str) -> RustReusableSelectorEntry {
    RustReusableSelectorEntry {
        selector: selector.to_string(),
        generation_fingerprint: "prior-gen".to_string(),
        status: rpytest_runner::TestStatus::Passed,
        coverage: RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
        test_binary_ids: vec![binary_id.to_string()],
    }
}

#[test]
fn classifier_reuses_only_unchanged_test_executables() {
    let prior = prior_state();
    let prior_entries = BTreeMap::from([
        ("alpha".to_string(), entry("alpha", "bin-a")),
        ("beta".to_string(), entry("beta", "bin-b")),
    ]);
    let current_selector_binaries = BTreeMap::from([
        ("alpha".to_string(), vec!["bin-a".to_string()]),
        ("beta".to_string(), vec!["bin-b".to_string()]),
    ]);
    let current_binary_digest = BTreeMap::from([
        ("bin-a".to_string(), "aaaaaaaaaaaaaaaa".to_string()),
        ("bin-b".to_string(), "changed000000000".to_string()),
    ]);

    let (retained, invalid) = classify_incremental_rust_selectors(
        &prior.selectors,
        &prior,
        &prior_entries,
        &current_selector_binaries,
        &current_binary_digest,
    );

    assert_eq!(retained, vec!["alpha".to_string()]);
    assert_eq!(invalid, vec!["beta".to_string()]);
}

#[test]
fn classifier_invalidates_remapped_selector_binary() {
    let prior = prior_state();
    let prior_entries = BTreeMap::from([("alpha".to_string(), entry("alpha", "bin-a"))]);
    let current_selector_binaries =
        BTreeMap::from([("alpha".to_string(), vec!["bin-b".to_string()])]);
    let current_binary_digest =
        BTreeMap::from([("bin-b".to_string(), "bbbbbbbbbbbbbbbb".to_string())]);

    let (retained, invalid) = classify_incremental_rust_selectors(
        &["alpha".to_string()],
        &prior,
        &prior_entries,
        &current_selector_binaries,
        &current_binary_digest,
    );

    assert!(retained.is_empty());
    assert_eq!(invalid, vec!["alpha".to_string()]);
}

#[test]
fn classifier_invalidates_selector_when_any_owned_binary_changes() {
    let prior = prior_state();
    let mut alpha = entry("alpha", "bin-a");
    alpha.test_binary_ids.push("bin-b".to_string());
    let prior_entries = BTreeMap::from([("alpha".to_string(), alpha)]);
    let current_selector_binaries = BTreeMap::from([(
        "alpha".to_string(),
        vec!["bin-a".to_string(), "bin-b".to_string()],
    )]);
    let current_binary_digest = BTreeMap::from([
        ("bin-a".to_string(), "aaaaaaaaaaaaaaaa".to_string()),
        ("bin-b".to_string(), "changed000000000".to_string()),
    ]);

    let (retained, invalid) = classify_incremental_rust_selectors(
        &["alpha".to_string()],
        &prior,
        &prior_entries,
        &current_selector_binaries,
        &current_binary_digest,
    );

    assert!(retained.is_empty());
    assert_eq!(invalid, vec!["alpha".to_string()]);
}
