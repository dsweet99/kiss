//! Migrate complete v1 Python derived state into a v2 generation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

use super::evidence::{PopulationEvidence, SelectorEvidence};
use super::identity::population_plan_for_selectors;
use super::publish::publish_python_population_generation;
use crate::test_runner::python_coverage_index::coverage_snapshot::try_load_python_coverage_snapshot;
use crate::test_runner::python_coverage_index::manifest::{
    PYTHON_COVERAGE_ENV_KEYS, read_python_population_manifest,
    stored_python_universe_population,
};
use crate::test_runner::python_coverage_index::storage::{load_current_python_coverage_index, python_coverage_cache_root};
use super::types::{GenerationReason, TimingCacheDisposition};
use rpytest_runner::TestStatus;

pub(crate) fn try_migrate_complete_v1_generation(
    repo_root: &Path,
    test_args: &[String],
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
) -> Result<Option<String>, String> {
    let _ = is_indexable;
    let Some(bundle) = load_complete_v1_bundle(repo_root, test_args)? else {
        return Ok(None);
    };
    let plan = population_plan_for_selectors(repo_root, &bundle.selectors, test_args)?;
    let Some(evidence) = evidence_from_v1_bundle(&plan.selectors, &bundle) else {
        return Ok(None);
    };
    publish_migrated_generation(repo_root, &plan, &evidence)
}

pub(super) struct V1Bundle {
    pub(super) selectors: Vec<String>,
    pub(super) coverage: BTreeMap<String, BTreeSet<u32>>,
    pub(super) index: BTreeMap<String, BTreeSet<String>>,
    pub(super) durations: BTreeMap<String, Duration>,
}

fn load_complete_v1_bundle(
    repo_root: &Path,
    test_args: &[String],
) -> Result<Option<V1Bundle>, String> {
    let population =
        stored_python_universe_population(repo_root, test_args, PYTHON_COVERAGE_ENV_KEYS);
    let manifest = read_python_population_manifest(repo_root);
    let coverage = try_load_python_coverage_snapshot(repo_root);
    let index = load_current_python_coverage_index(repo_root);
    let durations = load_v1_durations_for_migrate(repo_root, test_args, &population);
    let (Some(population), Some(manifest), Some(coverage), Some(index), Some(durations)) =
        (population, manifest, coverage, index, durations)
    else {
        return Ok(None);
    };
    if manifest.schema_version != crate::test_runner::python_coverage_index::POPULATION_SCHEMA_VERSION
        || durations.len() != population.selectors.len()
    {
        return Ok(None);
    }
    Ok(Some(V1Bundle {
        selectors: population.selectors,
        coverage,
        index,
        durations: durations.into_iter().collect(),
    }))
}

/// Prefer the durations sidecar; otherwise probe entries without requiring Passed.
fn load_v1_durations_for_migrate(
    repo_root: &Path,
    test_args: &[String],
    population: &Option<crate::test_runner::python_coverage_index::manifest::StoredPythonPopulation>,
) -> Option<Vec<(String, Duration)>> {
    let population = population.as_ref()?;
    let manifest = read_python_population_manifest(repo_root)?;
    let cache_root = python_coverage_cache_root(repo_root).ok()?;
    if let Some(cached) =
        crate::test_runner::python_coverage_index::population_durations::try_load_population_durations(&cache_root, &manifest)
    {
        return Some(cached);
    }
    crate::test_runner::python_coverage_index::population_durations::load_durations_from_entry_probes_allow_non_passed(
        repo_root,
        test_args,
        &population.selectors,
    )
}

pub(super) fn evidence_from_v1_bundle(
    selectors: &[String],
    bundle: &V1Bundle,
) -> Option<PopulationEvidence> {
    let mut evidence = PopulationEvidence::from_ordered_selectors(selectors);
    for selector in selectors {


        let cov = selector_coverage_from_index(selector, &bundle.index, &bundle.coverage);
        evidence.absorb_selector(SelectorEvidence {
            selector: selector.clone(),
            raw_status: TestStatus::Passed,
            effective_status: TestStatus::Passed,
            duration: bundle.durations.get(selector).copied(),
            cache_disposition: TimingCacheDisposition::Unknown,
            reason: None,
            coverage: cov,
        });
    }
    evidence.complete.then_some(evidence)
}

fn publish_migrated_generation(
    repo_root: &Path,
    plan: &super::types::PythonPopulationPlan,
    evidence: &PopulationEvidence,
) -> Result<Option<String>, String> {
    let cache_root = python_coverage_cache_root(repo_root)?;
    if !cache_root.join("population.json").is_file() {
        return Ok(None);
    }
    let generation_id = publish_python_population_generation(
        repo_root,
        plan,
        evidence,
        GenerationReason::Migration,
    )?;
    let _ = fs::remove_file(cache_root.join("coverage_snapshot.json"));
    let _ = fs::remove_file(cache_root.join("population_durations.json"));
    Ok(Some(generation_id))
}

pub(super) fn selector_coverage_from_index(
    selector: &str,
    index: &BTreeMap<String, BTreeSet<String>>,
    coverage: &BTreeMap<String, BTreeSet<u32>>,
) -> BTreeMap<String, BTreeSet<u32>> {
    let mut out = BTreeMap::new();
    for (file, selectors) in index {
        if !selectors.contains(selector) {
            continue;
        }
        if let Some(lines) = coverage.get(file) {
            out.insert(file.clone(), lines.clone());
        }
    }
    out
}

#[cfg(test)]
#[path = "migrate_test.rs"]
mod tests;
