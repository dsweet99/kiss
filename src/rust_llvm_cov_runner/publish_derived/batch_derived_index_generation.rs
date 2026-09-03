use crate::rust_llvm_cov_runner::plan::batch_fingerprint::RustCoverageBatchIdentity;
use std::path::Path;

use super::{load_current_population_state, RustGenerationCoverageSnapshot, RustPopulationState};

pub fn load_current_generation_coverage_from_passing_entries(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: Option<&[String]>,
) -> Option<RustGenerationCoverageSnapshot> {
    let population = load_current_population_state(cache_root, source_root, identity, selectors)?;
    crate::rust_llvm_cov_runner::population_entries_all_pass(cache_root, &population)
        .then_some(())?;
    generation_coverage_from_population(cache_root, source_root, population)
}

pub(super) fn generation_coverage_from_population(
    cache_root: &Path,
    source_root: &Path,
    population: RustPopulationState,
) -> Option<RustGenerationCoverageSnapshot> {
    let entries = crate::rust_llvm_cov_runner::publish_derived::batch_derived_snapshot::load_manifest_generation_entries(
        cache_root,
        source_root,
        &population,
    )?;
    let snapshot_identity =
        crate::rust_llvm_cov_runner::publish_derived::batch_derived_snapshot::stable_generation_coverage_identity(
            &population,
            &entries,
        );
    Some(RustGenerationCoverageSnapshot {
        identity: snapshot_identity,
        covered_lines: entries,
        population,
    })
}
