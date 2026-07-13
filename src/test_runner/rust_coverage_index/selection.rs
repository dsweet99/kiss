use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::test_runner::coverage_decision::{CoverageFreshness, RustSelectionBasis};

use super::{
    changed_line_rels, load_current_rust_population_state, load_reusable_prior_rust_population_state,
    repo_relative_path, selectors_by_changed_file_line, selectors_for_source_paths,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedRustPopulation {
    pub(crate) freshness: CoverageFreshness,
    pub(crate) basis: RustSelectionBasis,
    pub(crate) state: Option<rust_llvm_cov_runner::RustPopulationState>,
}

pub(crate) fn resolve_rust_population_state(
    repo_root: &Path,
    ignore: &[String],
    rust_source_paths: &[PathBuf],
    test_args: &[String],
) -> Result<ResolvedRustPopulation, String> {
    if rust_source_paths.is_empty() {
        return Ok(ResolvedRustPopulation {
            freshness: CoverageFreshness::Fresh,
            basis: RustSelectionBasis::Current,
            state: None,
        });
    }
    let universe = super::super::runners::enumerate_workspace_rust_selectors(repo_root, ignore)?;
    let current = load_current_rust_population_state(repo_root, Some(&universe), test_args);
    if current.is_some() {
        return Ok(ResolvedRustPopulation {
            freshness: CoverageFreshness::Fresh,
            basis: RustSelectionBasis::Current,
            state: current,
        });
    }
    let reusable = load_reusable_prior_rust_population_state(repo_root, Some(&universe), test_args);
    if reusable.is_some() {
        return Ok(ResolvedRustPopulation {
            freshness: CoverageFreshness::ReusablePrior,
            basis: RustSelectionBasis::ReusablePrior,
            state: reusable,
        });
    }
    Ok(ResolvedRustPopulation {
        freshness: CoverageFreshness::Stale,
        basis: RustSelectionBasis::Population,
        state: None,
    })
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
    match resolved.basis {
        RustSelectionBasis::Current => select_current_basis_rust_source_selectors(
            repo_root,
            rust_source_paths,
            rust_changed_lines,
            test_args,
            resolved.state.as_ref()?,
        ),
        RustSelectionBasis::ReusablePrior => select_reusable_prior_rust_source_selectors(
            repo_root,
            rust_source_paths,
            resolved.state.as_ref()?,
        ),
        RustSelectionBasis::Population => None,
    }
}

fn select_current_basis_rust_source_selectors(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    _test_args: &[String],
    population: &rust_llvm_cov_runner::RustPopulationState,
) -> Option<BTreeSet<String>> {
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

fn select_reusable_prior_rust_source_selectors(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    population: &rust_llvm_cov_runner::RustPopulationState,
) -> Option<BTreeSet<String>> {
    let mut selectors = BTreeSet::new();
    for source_path in rust_source_paths {
        if !source_path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        {
            return None;
        }
        let rel = repo_relative_path(repo_root, source_path)?;
        let file_selectors = population.line_index.get(&rel)?;
        if file_selectors.is_empty() {
            return None;
        }
        selectors.extend(file_selectors.iter().cloned());
    }
    Some(selectors)
}

#[cfg(test)]
mod coverage_witness {
    use super::ResolvedRustPopulation;
    use crate::test_runner::coverage_decision::{CoverageFreshness, RustSelectionBasis};

    #[test]
    fn witness_resolved_population_struct() {
        let resolved = ResolvedRustPopulation {
            freshness: CoverageFreshness::ReusablePrior,
            basis: RustSelectionBasis::ReusablePrior,
            state: None,
        };
        assert_eq!(resolved.basis, RustSelectionBasis::ReusablePrior);
    }
}
