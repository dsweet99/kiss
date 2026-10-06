use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::test_runner::lang_iface::{ExecutionWitness, GenerationIds, StoredCoverage};
use crate::test_runner::rust_coverage_index::{
    load_current_rust_population_state, selectors_for_source_paths,
};

pub(super) fn stored_witness(repo_root: &Path) -> Option<ExecutionWitness> {
    super::try_load_rust_execution_witness(repo_root).ok()
}

pub(super) fn historical_covering_selectors(
    repo_root: &Path,
    keys: &[String],
    abs: &[PathBuf],
) -> BTreeSet<String> {
    let mut selectors = BTreeSet::new();
    let Some(pop) = load_current_rust_population_state(repo_root, None, &[]) else {
        return selectors;
    };
    for key in keys {
        if let Some(found) = pop.line_index.get(key) {
            selectors.extend(found.iter().cloned());
        }
    }
    if let Some(found) = selectors_for_source_paths(repo_root, abs, &pop.line_index) {
        selectors.extend(found);
    }
    selectors
}

pub(super) fn indexes_path(repo_root: &Path, keys: &[String]) -> bool {
    load_current_rust_population_state(repo_root, None, &[])
        .is_some_and(|pop| keys.iter().any(|key| pop.line_index.contains_key(key)))
}

/// The coverable lines the holding Rust records covered; lines of in-file test
/// modules are not coverable and are left out.
pub(super) fn stored_coverage(repo_root: &Path) -> StoredCoverage {
    let mut stored = StoredCoverage::default();
    let Ok(witness) = super::try_load_rust_execution_witness(repo_root) else {
        return stored;
    };
    let files: Vec<PathBuf> = witness
        .covered_lines
        .keys()
        .map(|rel| repo_root.join(rel))
        .filter(|path| path.is_file())
        .collect();
    let Ok(facts) = crate::analyze::line_coverage::CoverageSourceFacts::from_files(&[], &files)
    else {
        return stored;
    };
    for (abs, coverable) in facts.coverable_map() {
        let Ok(rel) = abs.strip_prefix(repo_root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let Some(lines) = witness.covered_lines.get(&rel) else {
            continue;
        };
        let covered: BTreeSet<u32> = lines
            .iter()
            .copied()
            .filter(|line| usize::try_from(*line).is_ok_and(|line| coverable.contains(&line)))
            .collect();
        if !covered.is_empty() {
            stored.covered.insert(rel, covered);
        }
    }
    stored
}

pub(super) fn runner_identity_part(repo_root: &Path) -> Option<serde_json::Value> {
    let witness = super::try_load_rust_execution_witness(repo_root).ok()?;
    Some(serde_json::json!({
        "lang": "rust",
        "identity_digest": witness.identity_digest,
    }))
}

pub(super) fn generation_ids(repo_root: &Path) -> GenerationIds {
    GenerationIds {
        witness: super::try_load_rust_execution_witness(repo_root)
            .ok()
            .map(|witness| witness.generation_id),
    }
}
