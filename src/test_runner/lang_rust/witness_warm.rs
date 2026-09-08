use std::collections::BTreeSet;
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
    let population = kiss::rust_llvm_cov_runner::current_population_manifest_state(
        &cache_root,
        &identity,
    )
    .or_else(|| {
        kiss::rust_llvm_cov_runner::load_current_population_state(
            &cache_root,
            repo_root,
            &identity,
            None,
        )
    });
    let binaries_are_current = population.as_ref().map_or_else(
        || {
            kiss::rust_llvm_cov_runner::current_population_manifest_test_binaries_match(
                &cache_root,
                repo_root,
                &identity,
            )
            .unwrap_or(false)
        },
        |population| kiss::rust_llvm_cov_runner::current_test_binaries_match(repo_root, population),
    );
    let invalidation = if binaries_are_current {
        classify_ordinary_source_delta(&cache_root, repo_root, &identity)
    } else {
        OrdinarySourceInvalidation::All
    };
    let mut misses = planned_misses_for(planned_selectors, invalidation);
    if let Some(population) = population
        && binaries_are_current
    {
        let planned: BTreeSet<_> = planned_selectors.iter().map(String::as_str).collect();
        misses.extend(
            kiss::rust_llvm_cov_runner::population_nonpassed_selectors(&cache_root, &population)
                .into_iter()
                .filter(|selector| planned.contains(selector.as_str())),
        );
    }
    misses.sort();
    misses.dedup();
    Ok(misses)
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
    if !kiss::rust_llvm_cov_runner::current_population_manifest_test_binaries_match(
        &cache_root,
        repo_root,
        identity,
    )
    .unwrap_or(false)
    {
        return RustWarmDecision::Miss;
    }
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
            return warm_from_affected_selectors(
                repo_root,
                planned_selectors,
                identity,
                gate,
                affected,
            );
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

fn warm_from_affected_selectors(
    repo_root: &Path,
    planned_selectors: &[String],
    identity: &RustCoverageBatchIdentity,
    gate: &GateConfig,
    affected: BTreeSet<String>,
) -> RustWarmDecision {
    let mut misses = planned_misses_for(
        planned_selectors,
        OrdinarySourceInvalidation::Selectors(affected),
    );
    if let Some(witness_misses) = rust_miss_selectors(repo_root, planned_selectors, identity, gate)
    {
        for selector in witness_misses {
            if !misses.iter().any(|existing| existing == &selector) {
                misses.push(selector);
            }
        }
    }
    if misses.is_empty() {
        return try_warm_rust_cached_summary(repo_root, planned_selectors, identity, gate)
            .map(Box::new)
            .map_or(RustWarmDecision::Miss, RustWarmDecision::Warm);
    }
    if misses.len() >= planned_selectors.len() {
        return RustWarmDecision::Miss;
    }
    RustWarmDecision::RunMisses(misses)
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
    fn source_delta_misses_uses_full_population_for_selective_plan() {
        use kiss::rpytest_runner::TestStatus;
        use kiss::rust_llvm_cov_runner::{RustLineCoverage, RustTestBinaryIdentity};
        use std::collections::{BTreeMap, BTreeSet};

        let tmp = tempfile::tempdir().unwrap();
        write_minimal_repo(tmp.path());
        let lib = tmp.path().join("src/lib.rs");
        let coverage = RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        };
        crate::test_runner::rust_coverage_index::write_test_entry(
            tmp.path(),
            "alpha",
            "alpha",
            TestStatus::Passed,
            coverage.clone(),
        );
        crate::test_runner::rust_coverage_index::write_test_entry(
            tmp.path(),
            "beta",
            "beta",
            TestStatus::Failed,
            coverage,
        );
        let (mut req, tools) =
            crate::test_runner::rust_coverage_index::resolved_rust_batch_request_parts(
                tmp.path(),
                &[],
            )
            .unwrap();
        req.logical_selectors = vec!["alpha".into(), "beta".into()];
        req.population_publication_selectors = Some(req.logical_selectors.clone());
        let identity = kiss::rust_llvm_cov_runner::batch_identity(&req, &tools).unwrap();
        let executable = tmp.path().join("target/test-bin");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"binary").unwrap();
        let binary_digest = b"binary"
            .iter()
            .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
            });
        kiss::rust_llvm_cov_runner::publish_derived_state_with_binaries(
            &req,
            &tools,
            &identity,
            &req.logical_selectors,
            &[RustTestBinaryIdentity {
                id: "test-bin".into(),
                executable: executable.to_string_lossy().to_string(),
                digest: format!("{binary_digest:016x}"),
            }],
            true,
        )
        .unwrap();

        assert_eq!(
            rust_source_delta_misses(tmp.path(), &["beta".into()], &[]).unwrap(),
            vec!["beta".to_string()]
        );
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
