use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::test_runner::coverage_decision::{CoverageFreshness, RustSelectionBasis};

use super::{
    changed_line_rels, current_rust_coverage_batch_identity, repo_relative_path,
    rust_coverage_cache_root, selectors_by_changed_file_line, selectors_for_source_paths,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedRustPopulation {
    pub(crate) freshness: CoverageFreshness,
    pub(crate) basis: RustSelectionBasis,
    pub(crate) state: Option<rust_llvm_cov_runner::RustPopulationState>,
    pub(crate) snapshot_delta: Option<rust_llvm_cov_runner::RustSnapshotDelta>,
}

pub(crate) fn resolve_rust_population_state(
    repo_root: &Path,
    ignore: &[String],
    rust_source_paths: &[PathBuf],
    test_args: &[String],
) -> Result<ResolvedRustPopulation, String> {
    let _ = rust_source_paths;
    let universe = super::super::runners::enumerate_workspace_rust_selectors(repo_root, ignore)?;
    let identity = current_rust_coverage_batch_identity(repo_root, test_args)?;
    let cache_root = rust_coverage_cache_root(repo_root);
    let current = rust_llvm_cov_runner::load_current_population_state(
        &cache_root,
        repo_root,
        &identity,
        Some(&universe),
    );
    if current.is_some() {
        return Ok(ResolvedRustPopulation {
            freshness: CoverageFreshness::Fresh,
            basis: RustSelectionBasis::Current,
            state: current,
            snapshot_delta: None,
        });
    }
    let partial_current = rust_llvm_cov_runner::load_current_population_state(
        &cache_root,
        repo_root,
        &identity,
        None,
    );
    if let Some(partial_current) = partial_current
        && current_partial_population_covers_selection(
            repo_root,
            rust_source_paths,
            &BTreeMap::new(),
            test_args,
            &partial_current,
        )
    {
        return Ok(ResolvedRustPopulation {
            freshness: CoverageFreshness::Fresh,
            basis: RustSelectionBasis::Current,
            state: Some(partial_current),
            snapshot_delta: None,
        });
    }
    let reusable = rust_llvm_cov_runner::load_reusable_prior_population_state(
        &cache_root,
        repo_root,
        Some(&universe),
        &identity.selection_context_fingerprint,
    );
    if let Some(reusable) = reusable {
        let delta = rust_llvm_cov_runner::reusable_snapshot_delta(
            repo_root,
            &reusable.ordinary_source_digests,
            &identity.ordinary_source_digests,
        );
        if delta == rust_llvm_cov_runner::RustSnapshotDelta::StructuralChange {
            return Ok(ResolvedRustPopulation {
                freshness: CoverageFreshness::Stale,
                basis: RustSelectionBasis::Population,
                state: None,
                snapshot_delta: Some(delta),
            });
        }
        return Ok(ResolvedRustPopulation {
            freshness: CoverageFreshness::ReusablePrior,
            basis: RustSelectionBasis::ReusablePrior,
            state: Some(reusable),
            snapshot_delta: Some(delta),
        });
    }
    Ok(ResolvedRustPopulation {
        freshness: CoverageFreshness::Stale,
        basis: RustSelectionBasis::Population,
        state: None,
        snapshot_delta: None,
    })
}

fn current_partial_population_covers_selection(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    test_args: &[String],
    population: &rust_llvm_cov_runner::RustPopulationState,
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
            rust_changed_lines,
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
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    population: &rust_llvm_cov_runner::RustPopulationState,
) -> Option<BTreeSet<String>> {
    let line_selectors_by_file = if rust_changed_lines.is_empty() {
        BTreeMap::new()
    } else {
        selectors_by_changed_file_line(
            repo_root,
            &changed_line_rels(repo_root, rust_changed_lines),
            &population.generation_fingerprint,
        )
    };
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
        let selected_for_file = line_selectors_by_file
            .get(&rel)
            .filter(|selectors| !selectors.is_empty())
            .unwrap_or(file_selectors);
        selectors.extend(selected_for_file.iter().cloned());
    }
    Some(selectors)
}

#[cfg(test)]
mod coverage_witness {
    use super::{ResolvedRustPopulation, current_partial_population_covers_selection};
    use crate::test_runner::coverage_decision::{CoverageFreshness, RustSelectionBasis};
    use rust_llvm_cov_runner::RustPopulationState;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn witness_resolved_population_struct() {
        let resolved = ResolvedRustPopulation {
            freshness: CoverageFreshness::ReusablePrior,
            basis: RustSelectionBasis::ReusablePrior,
            state: None,
            snapshot_delta: None,
        };
        assert_eq!(resolved.basis, RustSelectionBasis::ReusablePrior);
    }

    #[test]
    fn partial_current_population_must_exactly_cover_changed_source_selection() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src").join("lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "pub fn value() -> u32 { 1 }\n").unwrap();
        let population = RustPopulationState {
            input_fingerprint: "input".to_string(),
            generation_fingerprint: "generation".to_string(),
            selection_context_fingerprint: "selection".to_string(),
            entries_fingerprint: "entries".to_string(),
            selectors: vec!["tests::covers_src".to_string()],
            line_index: BTreeMap::from([(
                "src/lib.rs".to_string(),
                BTreeSet::from(["tests::covers_src".to_string()]),
            )]),
            ordinary_source_digests: BTreeMap::new(),
            test_binaries: BTreeMap::new(),
        };

        assert!(current_partial_population_covers_selection(
            tmp.path(),
            std::slice::from_ref(&src),
            &BTreeMap::new(),
            &[],
            &population
        ));
        let mut extra_manifest_selector = population.clone();
        extra_manifest_selector
            .selectors
            .push("tests::not_selected".to_string());
        assert!(!current_partial_population_covers_selection(
            tmp.path(),
            &[src],
            &BTreeMap::new(),
            &[],
            &extra_manifest_selector
        ));
    }
}
