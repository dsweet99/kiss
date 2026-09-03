use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::super::{
    changed_line_rels, repo_relative_path, selectors_by_changed_file_line,
};
use super::select_check_aggregate_current_basis;

pub(super) fn select_reusable_prior_rust_source_selectors(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    population: &kiss::rust_llvm_cov_runner::RustPopulationState,
) -> Option<BTreeSet<String>> {
    if kiss::rust_llvm_cov_runner::is_check_aggregate_population(population) {
        for source_path in rust_source_paths {
            if !source_path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
            {
                return None;
            }
        }
        return select_check_aggregate_current_basis(
            repo_root,
            rust_source_paths,
            rust_changed_lines,
            population,
        );
    }
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
