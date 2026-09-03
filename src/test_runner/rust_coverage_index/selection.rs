use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::test_runner::coverage_decision::{CoverageFreshness, SelectionBasis};

use super::{
    changed_line_rels, current_rust_coverage_batch_identity, repo_relative_path,
    rust_coverage_cache_root, selectors_by_changed_file_line, selectors_for_source_paths,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ResolvedRustPopulation {
    Current {
        state: kiss::rust_llvm_cov_runner::RustPopulationState,
    },
    ReusablePrior {
        state: kiss::rust_llvm_cov_runner::RustPopulationState,
        delta: kiss::rust_llvm_cov_runner::RustSnapshotDelta,
    },
    StructuralStale,
    ColdStale,
}

impl ResolvedRustPopulation {
    pub(crate) fn freshness(&self) -> CoverageFreshness {
        match self {
            Self::Current { .. } => CoverageFreshness::Fresh,
            Self::ReusablePrior { .. } => CoverageFreshness::ReusablePrior,
            Self::StructuralStale | Self::ColdStale => CoverageFreshness::Stale,
        }
    }

    pub(crate) fn basis(&self) -> SelectionBasis {
        match self {
            Self::Current { .. } => SelectionBasis::Current,
            Self::ReusablePrior { .. } => SelectionBasis::ReusablePrior,
            Self::StructuralStale | Self::ColdStale => SelectionBasis::Population,
        }
    }

    pub(crate) fn state(&self) -> Option<&kiss::rust_llvm_cov_runner::RustPopulationState> {
        match self {
            Self::Current { state } | Self::ReusablePrior { state, .. } => Some(state),
            Self::StructuralStale | Self::ColdStale => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolveRustPopulationArgs<'a> {
    pub repo_root: &'a Path,
    pub ignore: &'a [String],
    pub rust_source_paths: &'a [PathBuf],
    pub rust_changed_lines: &'a BTreeMap<PathBuf, BTreeSet<u32>>,
    pub expected_selectors: Option<&'a [String]>,
    pub test_args: &'a [String],
}

#[cfg(test)]
static EMPTY_CHANGED_LINES: BTreeMap<PathBuf, BTreeSet<u32>> = BTreeMap::new();

impl<'a> ResolveRustPopulationArgs<'a> {
    #[cfg(test)]
    pub(crate) fn for_paths(repo_root: &'a Path, rust_source_paths: &'a [PathBuf]) -> Self {
        Self {
            repo_root,
            ignore: &[],
            rust_source_paths,
            rust_changed_lines: &EMPTY_CHANGED_LINES,
            expected_selectors: None,
            test_args: &[],
        }
    }
}

pub(crate) fn resolve_rust_population_state(
    args: ResolveRustPopulationArgs<'_>,
) -> Result<ResolvedRustPopulation, String> {
    let identity = current_rust_coverage_batch_identity(args.repo_root, args.test_args)?;
    let cache_root = rust_coverage_cache_root(args.repo_root);
    if let Some(state) = load_exact_current_population(&cache_root, &identity, &args) {
        return Ok(ResolvedRustPopulation::Current { state });
    }
    if let Some(state) = load_partial_current_population(&cache_root, &identity, &args) {
        return Ok(ResolvedRustPopulation::Current { state });
    }
    load_reusable_or_stale(&cache_root, &identity, &args)
}

fn load_exact_current_population(
    cache_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    args: &ResolveRustPopulationArgs<'_>,
) -> Option<kiss::rust_llvm_cov_runner::RustPopulationState> {
    let expected = args.expected_selectors?;
    kiss::rust_llvm_cov_runner::load_current_population_state(
        cache_root,
        args.repo_root,
        identity,
        Some(expected),
    )
}

fn load_partial_current_population(
    cache_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    args: &ResolveRustPopulationArgs<'_>,
) -> Option<kiss::rust_llvm_cov_runner::RustPopulationState> {
    let state = kiss::rust_llvm_cov_runner::load_current_population_state(
        cache_root,
        args.repo_root,
        identity,
        None,
    )?;
    if args.expected_selectors.is_none() {
        return Some(state);
    }
    let covers = current_partial_population_covers_selection(
        args.repo_root,
        args.rust_source_paths,
        args.rust_changed_lines,
        args.test_args,
        &state,
    );
    covers.then_some(state)
}

fn load_reusable_or_stale(
    cache_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    args: &ResolveRustPopulationArgs<'_>,
) -> Result<ResolvedRustPopulation, String> {
    let universe =
        match crate::test_runner::workspace_selector_cache::load_cached_rust_workspace_selectors(
            args.repo_root,
            args.ignore,
        ) {
            Some(selectors) => selectors,
            None => {
                let selectors = super::super::runners::enumerate_workspace_rust_selectors(
                    args.repo_root,
                    args.ignore,
                )?;
                crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
                    args.repo_root,
                    args.ignore,
                    &selectors,
                );
                selectors
            }
        };
    let reusable = kiss::rust_llvm_cov_runner::load_reusable_prior_population_state(
        cache_root,
        args.repo_root,
        Some(&universe),
        &identity.selection_context_fingerprint,
    );
    let Some(reusable) = reusable else {
        return Ok(ResolvedRustPopulation::ColdStale);
    };
    let delta = kiss::rust_llvm_cov_runner::reusable_snapshot_delta(
        args.repo_root,
        &reusable.ordinary_source_digests,
        &identity.ordinary_source_digests,
    );
    if delta == kiss::rust_llvm_cov_runner::RustSnapshotDelta::StructuralChange {
        return Ok(ResolvedRustPopulation::StructuralStale);
    }
    Ok(ResolvedRustPopulation::ReusablePrior {
        state: reusable,
        delta,
    })
}

fn current_partial_population_covers_selection(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    test_args: &[String],
    population: &kiss::rust_llvm_cov_runner::RustPopulationState,
) -> bool {
    if rust_source_paths.is_empty() {
        return false;
    }
    let Some(selected) = select_current_basis_rust_source_selectors(
        repo_root,
        rust_source_paths,
        rust_changed_lines,
        test_args,
        population,
    ) else {
        return false;
    };
    let manifest_selectors = population
        .selectors
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    selected == manifest_selectors
}

pub(crate) fn select_rust_source_selectors_for_basis(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    test_args: &[String],
    resolved: &ResolvedRustPopulation,
) -> Option<BTreeSet<String>> {
    if rust_source_paths.is_empty() {
        return Some(BTreeSet::new());
    }
    if std::env::var_os("KISS_PLAN_TRACE").is_some() {
        eprintln!(
            "KISS_PLAN_TRACE rust_basis={:?} check_agg={} changed_line_files={} sources={}",
            resolved.basis(),
            resolved
                .state()
                .is_some_and(kiss::rust_llvm_cov_runner::is_check_aggregate_population),
            rust_changed_lines.len(),
            rust_source_paths.len()
        );
    }
    match resolved {
        ResolvedRustPopulation::Current { state } => select_current_basis_rust_source_selectors(
            repo_root,
            rust_source_paths,
            rust_changed_lines,
            test_args,
            state,
        ),
        ResolvedRustPopulation::ReusablePrior { state, .. } => {
            select_reusable_prior_rust_source_selectors(
                repo_root,
                rust_source_paths,
                rust_changed_lines,
                state,
            )
        }
        ResolvedRustPopulation::StructuralStale | ResolvedRustPopulation::ColdStale => None,
    }
}

fn select_current_basis_rust_source_selectors(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    _test_args: &[String],
    population: &kiss::rust_llvm_cov_runner::RustPopulationState,
) -> Option<BTreeSet<String>> {
    if kiss::rust_llvm_cov_runner::is_check_aggregate_population(population) {
        return select_check_aggregate_current_basis(
            repo_root,
            rust_source_paths,
            rust_changed_lines,
            population,
        );
    }
    if !rust_changed_lines.is_empty() {
        let changed_rels = changed_line_rels(repo_root, rust_changed_lines);
        let line_selectors_by_file = selectors_by_changed_file_line(
            repo_root,
            &changed_rels,
            &population.generation_fingerprint,
        );
        let index = &population.line_index;
        let mut selectors = BTreeSet::new();
        for source_path in rust_source_paths {
            let rel = repo_relative_path(repo_root, source_path)?;
            let Some(file_selectors) = index.get(&rel).filter(|selectors| !selectors.is_empty())
            else {
                continue;
            };
            let selected_for_file = line_selectors_by_file
                .get(&rel)
                .filter(|selectors| !selectors.is_empty())
                .cloned()
                .unwrap_or_else(|| file_selectors.clone());
            selectors.extend(selected_for_file);
        }
        return Some(selectors);
    }
    selectors_for_source_paths(repo_root, rust_source_paths, &population.line_index)
}

fn select_check_aggregate_current_basis(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    population: &kiss::rust_llvm_cov_runner::RustPopulationState,
) -> Option<BTreeSet<String>> {
    const LINE_PRECISE_FILE_LIMIT: usize = 1;
    if !rust_changed_lines.is_empty() && rust_changed_lines.len() <= LINE_PRECISE_FILE_LIMIT {
        let changed_rels = changed_line_rels(repo_root, rust_changed_lines);
        let line_selectors_by_file = selectors_by_changed_file_line(
            repo_root,
            &changed_rels,
            &population.generation_fingerprint,
        );
        let file_selectors = check_aggregate_file_selectors(repo_root, population);
        let mut selectors = BTreeSet::new();
        let mut saw_covered_file = false;
        for source_path in rust_source_paths {
            let rel = repo_relative_path(repo_root, source_path)?;
            if !population.line_index.contains_key(&rel) {
                return None;
            }
            saw_covered_file = true;
            if let Some(selected_for_file) = line_selectors_by_file
                .get(&rel)
                .filter(|selected| !selected.is_empty())
            {
                let narrowed = planned_check_aggregate_line_selectors(
                    selected_for_file,
                    &population.selectors,
                );
                if !narrowed.is_empty() {
                    selectors.extend(narrowed);
                    continue;
                }
            }
            let narrowed = file_selectors
                .get(&rel)
                .map(|selected| {
                    planned_check_aggregate_line_selectors(selected, &population.selectors)
                })
                .unwrap_or_default();
            if narrowed.is_empty() {
                return Some(population.selectors.iter().cloned().collect());
            }
            selectors.extend(narrowed);
        }
        if saw_covered_file {
            return Some(selectors);
        }
        return None;
    }
    select_check_aggregate_source_selectors(repo_root, rust_source_paths, population)
}

fn planned_check_aggregate_line_selectors(
    selected_for_file: &BTreeSet<String>,
    population_selectors: &[String],
) -> BTreeSet<String> {
    let planned: BTreeSet<String> = population_selectors.iter().cloned().collect();
    selected_for_file.intersection(&planned).cloned().collect()
}

fn select_check_aggregate_source_selectors(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    population: &kiss::rust_llvm_cov_runner::RustPopulationState,
) -> Option<BTreeSet<String>> {
    let plan_trace = std::env::var_os("KISS_PLAN_TRACE").is_some();
    let mark = std::time::Instant::now();
    let file_selectors = check_aggregate_file_selectors(repo_root, population);
    let mut out = BTreeSet::new();
    let mut saw_covered_file = false;
    for source_path in rust_source_paths {
        let rel = repo_relative_path(repo_root, source_path)?;
        if !population.line_index.contains_key(&rel) {
            return None;
        }
        saw_covered_file = true;
        let narrowed = file_selectors
            .get(&rel)
            .map(|selected| planned_check_aggregate_line_selectors(selected, &population.selectors))
            .unwrap_or_default();
        if narrowed.is_empty() {
            out.extend(population.selectors.iter().cloned());
        } else {
            out.extend(narrowed);
        }
    }
    if saw_covered_file {
        if plan_trace {
            eprintln!(
                "KISS_PLAN_TRACE check_agg_select_ms={} sources={} selectors={}",
                mark.elapsed().as_millis(),
                rust_source_paths.len(),
                out.len()
            );
        }
        return Some(out);
    }
    if plan_trace {
        eprintln!(
            "KISS_PLAN_TRACE check_agg_select_ms={} sources={} selectors=0",
            mark.elapsed().as_millis(),
            rust_source_paths.len()
        );
    }
    None
}

fn check_aggregate_file_selectors(
    repo_root: &Path,
    population: &kiss::rust_llvm_cov_runner::RustPopulationState,
) -> BTreeMap<String, BTreeSet<String>> {
    if !kiss::rust_llvm_cov_runner::current_test_binaries_match(repo_root, population) {
        return BTreeMap::new();
    }
    let Some(snapshot) = kiss::rust_llvm_cov_runner::load_reusable_prior_check_aggregate(
        &rust_coverage_cache_root(repo_root),
        repo_root,
        &population.selectors,
        &population.selection_context_fingerprint,
    ) else {
        return BTreeMap::new();
    };
    if snapshot.generation_fingerprint != population.generation_fingerprint {
        return BTreeMap::new();
    }
    kiss::rust_llvm_cov_runner::file_selector_index_from_validated(&snapshot)
}

#[path = "selection_reuse.rs"]
mod selection_reuse;
use selection_reuse::select_reusable_prior_rust_source_selectors;

#[cfg(test)]
#[path = "selection_coverage_witness_test.rs"]
mod coverage_witness;
