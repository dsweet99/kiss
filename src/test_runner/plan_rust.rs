use std::collections::BTreeSet;
use std::path::Path;

use crate::test_runner::execution_witness::{
    rust_identity_digest_from_batch, try_load_rust_execution_witness,
};
use crate::test_runner::lang_iface::identity_covers;
#[cfg(test)]
use crate::test_runner::lang_iface::{AcceptDecision, AcceptMode, accept_witness};
use kiss::GateConfig;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SelectorUniverseClass {
    pub deleted_candidates: Vec<String>,
    pub mandatory_misses: Vec<String>,
    pub intersection: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RustAllModePlan {
    pub planned: Vec<String>,
    pub population_required: bool,
    pub classification: SelectorUniverseClass,
}

#[cfg(test)]
pub(super) fn rust_population_current_for_all_selectors(
    repo_root: &Path,
    selectors: &[String],
    gate: &GateConfig,
) -> bool {
    rust_current_for_exact_universe(repo_root, selectors, gate).is_some()
}

pub(super) fn rust_plan_selectors(
    repo_root: &Path,
    selectors: Vec<String>,
    _gate: &GateConfig,
) -> RustAllModePlan {
    let planned = sort_dedup(selectors);
    let classification =
        classify_selector_universe(&planned, cached_selector_universe(repo_root).as_deref());
    let population_required = rust_all_mode_population_required(repo_root, &planned);
    RustAllModePlan {
        planned,
        population_required,
        classification,
    }
}

fn sort_dedup(mut selectors: Vec<String>) -> Vec<String> {
    selectors.sort();
    selectors.dedup();
    selectors
}

fn classify_selector_universe(
    discovered: &[String],
    cached: Option<&[String]>,
) -> SelectorUniverseClass {
    let discovered_set: BTreeSet<&str> = discovered.iter().map(String::as_str).collect();
    let cached_set: BTreeSet<&str> = cached
        .map(|cached| cached.iter().map(String::as_str).collect())
        .unwrap_or_default();
    SelectorUniverseClass {
        deleted_candidates: cached_set
            .difference(&discovered_set)
            .map(|selector| (*selector).to_string())
            .collect(),
        mandatory_misses: discovered_set
            .difference(&cached_set)
            .map(|selector| (*selector).to_string())
            .collect(),
        intersection: discovered_set
            .intersection(&cached_set)
            .map(|selector| (*selector).to_string())
            .collect(),
    }
}

fn cached_selector_universe(repo_root: &Path) -> Option<Vec<String>> {
    if let Some(pop) = population_cache_selectors(repo_root) {
        return Some(pop);
    }
    try_load_rust_execution_witness(repo_root)
        .ok()
        .map(|witness| witness.selectors)
        .filter(|selectors| !selectors.is_empty())
}

fn rust_all_mode_population_required(repo_root: &Path, planned: &[String]) -> bool {
    if planned.is_empty() {
        return false;
    }
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    if !cache_root.join("population.json").is_file()
        && !cache_root.join("execution_witness.json").is_file()
        && !cache_root.join("current_generation.json").is_file()
    {
        return true;
    }
    let identity_started = std::time::Instant::now();
    let identity = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        repo_root,
        &[],
    )
    .ok();
    crate::test_runner::emit_stage_time("rust_identity", identity_started.elapsed());
    let Some(identity) = identity.as_ref() else {
        return true;
    };
    if let Some(current) =
        kiss::rust_llvm_cov_runner::current_population_manifest_matches_universe(
            cache_root.as_path(),
            identity,
            planned,
        )
    {
        return !current;
    }
    if let Some(state) = kiss::rust_llvm_cov_runner::load_current_population_state(
        cache_root.as_path(),
        repo_root,
        identity,
        None,
    ) {
        return state.selectors != *planned;
    }
    !rust_witness_identity_compatible(repo_root, identity)
}

fn rust_witness_identity_compatible(
    repo_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
) -> bool {
    let Ok(witness) = try_load_rust_execution_witness(repo_root) else {
        return false;
    };
    if !witness.complete {
        return false;
    }
    let current = rust_identity_digest_from_batch(identity);
    identity_covers(&witness.identity_digest, &current)
}

fn population_cache_selectors(repo_root: &Path) -> Option<Vec<String>> {
    let path = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root)
        .join("population.json");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let selectors: Vec<String> = value
        .get("selectors")?
        .as_array()?
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect();
    (!selectors.is_empty()).then_some(selectors)
}

#[cfg(test)]
fn rust_current_for_exact_universe(
    repo_root: &Path,
    selectors: &[String],
    gate: &GateConfig,
) -> Option<Vec<String>> {
    let expected = sort_dedup(selectors.to_vec());
    if expected.is_empty() {
        return None;
    }
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    if !cache_root.join("population.json").is_file()
        && !cache_root.join("execution_witness.json").is_file()
        && !cache_root.join("current_generation.json").is_file()
    {
        return None;
    }
    let identity = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        repo_root,
        &[],
    )
    .ok()?;
    if kiss::rust_llvm_cov_runner::load_current_population_state(
        &cache_root,
        repo_root,
        &identity,
        Some(&expected),
    )
    .is_some()
    {
        return Some(expected);
    }
    rust_witness_accepts_full_universe(repo_root, &expected, &identity, gate).then_some(expected)
}

#[cfg(test)]
fn rust_witness_accepts_full_universe(
    repo_root: &Path,
    selectors: &[String],
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    _gate: &GateConfig,
) -> bool {
    let Ok(witness) = try_load_rust_execution_witness(repo_root) else {
        return false;
    };
    let current = rust_identity_digest_from_batch(identity);
    if !witness.complete {
        return false;
    }
    accept_witness(AcceptMode::All, selectors, &current, &witness) == AcceptDecision::Accept
}

#[cfg(test)]
#[path = "plan_rust_test.rs"]
mod tests;
