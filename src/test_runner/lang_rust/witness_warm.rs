use std::path::Path;

use kiss::GateConfig;
use kiss::rust_llvm_cov_runner::{
    OrdinarySourceInvalidation, RustCoverageBatchIdentity, classify_ordinary_source_delta,
};

use super::witness_store::{rust_miss_selectors, try_warm_rust_cached_summary};
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::rust_coverage_index::rust_coverage_cache_root;

pub(crate) fn rust_source_delta_misses(
    repo_root: &Path,
    planned_selectors: &[String],
    test_args: &[String],
) -> Result<Vec<String>, String> {
    let identity = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        repo_root, test_args,
    )?;
    let cache_root = rust_coverage_cache_root(repo_root);
    Ok(planned_misses_for(
        planned_selectors,
        classify_ordinary_source_delta(&cache_root, repo_root, &identity),
    ))
}

pub(crate) fn planned_misses_for(
    planned: &[String],
    invalidation: OrdinarySourceInvalidation,
) -> Vec<String> {
    match invalidation {
        OrdinarySourceInvalidation::All => planned.to_vec(),
        OrdinarySourceInvalidation::Selectors(affected) => planned
            .iter()
            .filter(|selector| affected.contains(*selector))
            .cloned()
            .collect(),
        OrdinarySourceInvalidation::None => Vec::new(),
    }
}

pub(crate) fn rust_warm_or_miss_selectors(
    repo_root: &Path,
    planned_selectors: &[String],
    identity: &RustCoverageBatchIdentity,
    gate: &GateConfig,
) -> RustWarmDecision {
    let cache_root = rust_coverage_cache_root(repo_root);
    apply_warm_invalidation(
        repo_root,
        planned_selectors,
        identity,
        gate,
        classify_ordinary_source_delta(&cache_root, repo_root, identity),
    )
}

pub(crate) fn apply_warm_invalidation(
    repo_root: &Path,
    planned_selectors: &[String],
    identity: &RustCoverageBatchIdentity,
    gate: &GateConfig,
    invalidation: OrdinarySourceInvalidation,
) -> RustWarmDecision {
    match invalidation {
        OrdinarySourceInvalidation::All => return RustWarmDecision::Miss,
        OrdinarySourceInvalidation::Selectors(affected) if !affected.is_empty() => {
            let mut misses = planned_misses_for(
                planned_selectors,
                OrdinarySourceInvalidation::Selectors(affected),
            );
            if let Some(witness_misses) =
                rust_miss_selectors(repo_root, planned_selectors, identity, gate)
            {
                for selector in witness_misses {
                    if !misses.iter().any(|existing| existing == &selector) {
                        misses.push(selector);
                    }
                }
            }
            if misses.is_empty() || misses.len() >= planned_selectors.len() {
                return RustWarmDecision::Miss;
            }
            return RustWarmDecision::RunMisses(misses);
        }
        OrdinarySourceInvalidation::None | OrdinarySourceInvalidation::Selectors(_) => {}
    }
    if let Some(summary) =
        try_warm_rust_cached_summary(repo_root, planned_selectors, identity, gate)
    {
        return RustWarmDecision::Warm(Box::new(summary));
    }
    match rust_miss_selectors(repo_root, planned_selectors, identity, gate) {
        Some(misses) if !misses.is_empty() && misses.len() < planned_selectors.len() => {
            RustWarmDecision::RunMisses(misses)
        }
        _ => RustWarmDecision::Miss,
    }
}

#[derive(Debug)]
pub(crate) enum RustWarmDecision {
    Warm(Box<SelectorExecutionSummary>),
    RunMisses(Vec<String>),
    Miss,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_minimal_repo(root: &Path) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(root.join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    }

    #[test]
    fn source_delta_misses_empty_root_is_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let planned = vec!["a".into()];
        if let Ok(misses) = rust_source_delta_misses(tmp.path(), &planned, &[]) {
            assert_eq!(misses, planned);
        }
    }

    #[test]
    fn source_delta_misses_all_without_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_repo(tmp.path());
        let planned = vec!["a".into(), "b".into()];
        let misses = rust_source_delta_misses(tmp.path(), &planned, &[]).unwrap();
        assert_eq!(misses, planned);
    }

    #[test]
    fn planned_misses_for_all_selectors_and_none() {
        let planned = vec!["a".into(), "b".into()];
        assert_eq!(
            planned_misses_for(&planned, OrdinarySourceInvalidation::All),
            planned
        );
        let only_a = std::collections::BTreeSet::from(["a".into()]);
        assert_eq!(
            planned_misses_for(&planned, OrdinarySourceInvalidation::Selectors(only_a)),
            vec!["a".to_string()]
        );
        assert!(planned_misses_for(&planned, OrdinarySourceInvalidation::None).is_empty());
    }
}
