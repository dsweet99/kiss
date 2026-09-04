use super::{
    current_partial_population_covers_selection, select_rust_source_selectors_for_basis,
    ResolvedRustPopulation,
};
use kiss::rust_llvm_cov_runner::RustPopulationState;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[test]
fn partial_cover_is_false_when_changed_source_is_outside_repo() {
    let root = std::path::Path::new(".");
    let outside = PathBuf::from("/tmp/kiss-not-in-repo.rs");
    let population = RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    assert!(!current_partial_population_covers_selection(
        root,
        std::slice::from_ref(&outside),
        &BTreeMap::from([(outside.clone(), BTreeSet::from([1]))]),
        &[],
        &population,
    ));
}

#[test]
fn plan_trace_emits_for_nonempty_stale_selection() {
    let _trace = crate::test_runner::TestEnvVarGuard::set("KISS_PLAN_TRACE", "1");
    let root = std::path::Path::new(".");
    assert!(
        select_rust_source_selectors_for_basis(
            root,
            &[PathBuf::from("src/lib.rs")],
            &BTreeMap::new(),
            &[],
            &ResolvedRustPopulation::ColdStale,
        )
        .is_none()
    );
}
