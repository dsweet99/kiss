use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{BackendCoverage, RuntimeCoverageLoadError, coverage_error};
use crate::test_runner::execution_witness::try_load_rust_execution_witness;
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity,
    repo_relative_coverage_file as rust_repo_relative_coverage_file, rust_coverage_cache_root,
};

pub(crate) fn load_rust_runtime_coverage(
    repo_root: &Path,
    ignore: &[String],
    gate: &kiss::GateConfig,
) -> Result<BackendCoverage, RuntimeCoverageLoadError> {
    let identity = current_rust_coverage_batch_identity(repo_root, &[]).map_err(|err| {
        coverage_error("Rust", &format!("stale/incompatible tool identity ({err})"))
    })?;
    let cache_root = rust_coverage_cache_root(repo_root);
    let selectors = rust_selectors_for_coverage_load(repo_root, ignore)?;
    if let Some(population) = kiss::rust_llvm_cov_runner::load_current_population_state(
        &cache_root,
        repo_root,
        &identity,
        Some(&selectors),
    ) && !kiss::rust_llvm_cov_runner::population_entries_all_pass(&cache_root, &population)
    {
        return Err(coverage_error(
            "Rust",
            "current execution evidence is stale or non-passing",
        ));
    }

    let witness_state = rust_witness_coverage_state(repo_root, &identity, &selectors, gate)?;
    if let Some(cov) = witness_state.coverage {
        return Ok(cov);
    }
    if witness_state.incomplete && !witness_state.matches_selector_universe {
        return Err(coverage_error(
            "Rust",
            "current execution witness is incomplete for a different selector universe",
        ));
    }
    if let Some(cov) = coverage_from_current_snapshots(
        repo_root,
        &cache_root,
        &identity,
        &selectors,
        witness_state.incomplete,
    )? {
        return Ok(cov);
    }

    Err(coverage_error(
        "Rust",
        "missing, stale/incompatible, incomplete, or malformed population",
    ))
}

fn coverage_from_current_snapshots(
    repo_root: &Path,
    cache_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selectors: &[String],
    current_witness_is_incomplete: bool,
) -> Result<Option<BackendCoverage>, RuntimeCoverageLoadError> {
    if let Some(snapshot) = kiss::rust_llvm_cov_runner::load_current_check_aggregate_snapshot(
        cache_root,
        repo_root,
        identity,
        Some(selectors),
    ) {
        return Ok(Some(backend_from_lines(
            snapshot.identity,
            snapshot.covered_lines,
        )));
    }
    if let Some(snapshot) = kiss::rust_llvm_cov_runner::load_current_generation_coverage_snapshot(
        cache_root,
        repo_root,
        identity,
        Some(selectors),
    ) {
        return Ok(Some(backend_from_lines(
            snapshot.identity,
            remap_rust_covered_lines(repo_root, snapshot.covered_lines)?,
        )));
    }
    if let Some(snapshot) =
        kiss::rust_llvm_cov_runner::load_current_generation_coverage_from_passing_entries(
            cache_root,
            repo_root,
            identity,
            Some(selectors),
        )
    {
        return Ok(Some(backend_from_lines(
            snapshot.identity,
            remap_rust_covered_lines(repo_root, snapshot.covered_lines)?,
        )));
    }
    if current_witness_is_incomplete {
        return Err(coverage_error(
            "Rust",
            "current execution witness is incomplete",
        ));
    }
    Ok(None)
}

fn rust_selectors_for_coverage_load(
    repo_root: &Path,
    ignore: &[String],
) -> Result<Vec<String>, RuntimeCoverageLoadError> {
    if let Some(rust_selectors) =
        crate::test_runner::workspace_selector_cache::load_cached_rust_workspace_selectors(
            repo_root, ignore,
        )
    {
        return Ok(rust_selectors);
    }
    let ids = crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
        .map_err(|err| coverage_error("Rust", &format!("selector discovery failed ({err})")))?;
    crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
        repo_root, ignore, &ids,
    );
    Ok(ids)
}

#[cfg(test)]
pub(super) fn try_load_rust_coverage_from_witness(
    repo_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selectors: &[String],
    gate: &kiss::GateConfig,
) -> Option<BackendCoverage> {
    let witness = try_load_rust_execution_witness(repo_root).ok()?;
    rust_coverage_from_witness(repo_root, identity, selectors, gate, &witness)
}

struct RustWitnessCoverageState {
    coverage: Option<BackendCoverage>,
    incomplete: bool,
    matches_selector_universe: bool,
}

fn rust_witness_coverage_state(
    repo_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selectors: &[String],
    gate: &kiss::GateConfig,
) -> Result<RustWitnessCoverageState, RuntimeCoverageLoadError> {
    let Ok(witness) = try_load_rust_execution_witness(repo_root) else {
        return Ok(RustWitnessCoverageState {
            coverage: None,
            incomplete: false,
            matches_selector_universe: false,
        });
    };
    if let Some(coverage) =
        rust_coverage_from_witness(repo_root, identity, selectors, gate, &witness)
    {
        return Ok(RustWitnessCoverageState {
            coverage: Some(coverage),
            incomplete: false,
            matches_selector_universe: true,
        });
    }
    let incomplete = witness_matches_identity(&witness, identity) && !witness.complete;
    let matches_selector_universe = !incomplete
        || witness.selectors.iter().collect::<BTreeSet<_>>()
            == selectors.iter().collect::<BTreeSet<_>>();
    Ok(RustWitnessCoverageState {
        coverage: None,
        incomplete,
        matches_selector_universe,
    })
}

fn rust_coverage_from_witness(
    repo_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selectors: &[String],
    gate: &kiss::GateConfig,
    witness: &crate::test_runner::lang_iface::ExecutionWitness,
) -> Option<BackendCoverage> {
    if witness.covered_lines.is_empty() {
        return None;
    }

    if !rust_witness_accepts_planned(repo_root, witness, identity, selectors, gate) {
        return None;
    }
    let covered: BTreeMap<String, BTreeSet<u32>> = witness
        .covered_lines
        .iter()
        .map(|(path, lines)| (path.clone(), lines.iter().copied().collect()))
        .collect();
    Some(backend_from_lines(witness.generation_id.clone(), covered))
}

fn witness_matches_identity(
    witness: &crate::test_runner::lang_iface::ExecutionWitness,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
) -> bool {
    let current = crate::test_runner::execution_witness::rust_identity_digest_from_batch(identity);
    crate::test_runner::lang_iface::identity_covers(&witness.identity_digest, &current)
}

fn rust_witness_accepts_planned(
    repo_root: &Path,
    witness: &crate::test_runner::lang_iface::ExecutionWitness,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selectors: &[String],
    _gate: &kiss::GateConfig,
) -> bool {
    use crate::test_runner::execution_witness::rust_identity_digest_from_batch;
    use crate::test_runner::lang_iface::{AcceptDecision, AcceptMode, accept_witness};
    let current = rust_identity_digest_from_batch(identity);
    if accept_witness(AcceptMode::All, selectors, &current, witness) != AcceptDecision::Accept {
        return false;
    }
    matches!(
        kiss::rust_llvm_cov_runner::classify_ordinary_source_delta(
            &rust_coverage_cache_root(repo_root),
            repo_root,
            identity,
        ),
        kiss::rust_llvm_cov_runner::OrdinarySourceInvalidation::None
    )
}

#[cfg(test)]
pub(super) fn try_load_rust_coverage_from_witness_prior(
    repo_root: &Path,
    cache_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selectors: &[String],
    gate: &kiss::GateConfig,
) -> Option<BackendCoverage> {
    let witness = try_load_rust_execution_witness(repo_root).ok()?;
    if !rust_witness_accepts_planned(repo_root, &witness, identity, selectors, gate) {
        return None;
    }

    let prior = kiss::rust_llvm_cov_runner::load_reusable_prior_check_aggregate(
        cache_root,
        repo_root,
        &witness.selectors,
        &identity.selection_context_fingerprint,
    )?;
    Some(backend_from_lines(
        prior.generation_fingerprint,
        prior.aggregate_covered_lines,
    ))
}

pub(super) fn backend_from_lines(
    identity: String,
    covered_lines: BTreeMap<String, BTreeSet<u32>>,
) -> BackendCoverage {
    BackendCoverage {
        identity,
        covered_lines,
    }
}

pub(super) fn remap_rust_covered_lines(
    repo_root: &Path,
    covered: BTreeMap<String, BTreeSet<u32>>,
) -> Result<BTreeMap<String, BTreeSet<u32>>, RuntimeCoverageLoadError> {
    let hashes = kiss::rust_llvm_cov_runner::load_ordinary_source_line_hashes(
        &rust_coverage_cache_root(repo_root),
    );
    let mut covered_lines = BTreeMap::<String, BTreeSet<u32>>::new();
    for (file, lines) in covered {
        let rel = rust_repo_relative_coverage_file(repo_root, &file)
            .ok_or_else(|| coverage_error("Rust", "malformed out-of-repository path"))?;
        let mapped = match hashes.as_ref().and_then(|stored| stored.get(&rel)) {
            Some(stored) => kiss::rust_llvm_cov_runner::remap_covered_file_lines(
                repo_root, &rel, stored, &lines,
            ),
            None => lines,
        };
        covered_lines.entry(rel).or_default().extend(mapped);
    }
    Ok(covered_lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn backend_from_lines_preserves_identity_and_map() {
        let cov = backend_from_lines(
            "id".into(),
            BTreeMap::from([("a.rs".into(), BTreeSet::from([1u32]))]),
        );
        assert_eq!(cov.identity, "id");
        assert!(cov.covered_lines.contains_key("a.rs"));
    }

    #[test]
    fn remap_rust_covered_lines_keeps_repo_relative_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), b"fn f() {}\n").unwrap();
        let abs = repo.join("src/lib.rs").to_string_lossy().to_string();
        let mapped =
            remap_rust_covered_lines(repo, BTreeMap::from([(abs, BTreeSet::from([1u32]))]))
                .unwrap();
        assert!(mapped.contains_key("src/lib.rs"));
    }

    #[test]
    fn remap_rust_covered_lines_rejects_out_of_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let err = remap_rust_covered_lines(
            tmp.path(),
            BTreeMap::from([("/tmp/outside.rs".into(), BTreeSet::from([1u32]))]),
        )
        .unwrap_err();
        assert!(err.reason.contains("malformed"));
    }

    #[test]
    fn witness_coverage_helpers_fail_closed_on_empty_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "g".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: Default::default(),
        };
        let selectors = vec!["a".into()];
        assert!(
            try_load_rust_coverage_from_witness(
                tmp.path(),
                &identity,
                &selectors,
                &kiss::GateConfig::default()
            )
            .is_none()
        );
        assert!(
            try_load_rust_coverage_from_witness_prior(
                tmp.path(),
                &tmp.path().join(".kiss/rust_llvm_cov_cache"),
                &identity,
                &selectors,
                &kiss::GateConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn witness_coverage_uses_embedded_covered_lines_when_accepted() {
        use crate::test_runner::execution_witness::{
            PublishRustWitness, WitnessScope, WitnessStatus, publish_rust_execution_witness,
        };
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "g".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: Default::default(),
        };
        let selectors = vec!["a".into(), "b".into()];
        let covered = BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1u32, 2u32]))]);
        let _ = publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &identity,
            scope: WitnessScope::Full,
            selectors: &selectors,
            statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: &[Some(10), Some(20)],
            covered_lines: &covered,
            complete: true,
            jobs: 1,
        })
        .unwrap();
        let loaded = try_load_rust_coverage_from_witness(
            tmp.path(),
            &identity,
            &selectors,
            &kiss::GateConfig::default(),
        )
        .unwrap();
        assert!(loaded.covered_lines.contains_key("src/lib.rs"));
        let mut drifted = identity.clone();
        drifted.generation_fingerprint = "g2".into();
        drifted.selection_context_fingerprint = "s2".into();
        assert!(
            try_load_rust_coverage_from_witness(
                tmp.path(),
                &drifted,
                &selectors,
                &kiss::GateConfig::default(),
            )
            .is_none()
        );

        let empty = BTreeMap::new();
        let _ = publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &identity,
            scope: WitnessScope::Full,
            selectors: &selectors,
            statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: &[Some(10), Some(20)],
            covered_lines: &empty,
            complete: true,
            jobs: 1,
        })
        .unwrap();
        assert!(
            try_load_rust_coverage_from_witness(
                tmp.path(),
                &identity,
                &selectors,
                &kiss::GateConfig::default()
            )
            .is_none()
        );
        let _ = publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &identity,
            scope: WitnessScope::Full,
            selectors: &selectors,
            statuses: &[WitnessStatus::Failed, WitnessStatus::Passed],
            durations_ns: &[Some(10), Some(20)],
            covered_lines: &covered,
            complete: false,
            jobs: 1,
        })
        .unwrap();
        let incomplete =
            crate::test_runner::execution_witness::try_load_rust_execution_witness(tmp.path())
                .unwrap();
        assert!(!incomplete.complete);
        assert!(witness_matches_identity(&incomplete, &identity));
    }

    #[test]
    fn witness_coverage_rejects_enumerator_extras() {
        use crate::test_runner::execution_witness::{
            PublishRustWitness, WitnessScope, WitnessStatus, publish_rust_execution_witness,
        };
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "g".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: Default::default(),
        };
        let witness_selectors = vec!["a".into(), "b".into()];
        let enumerator = vec!["a".into(), "b".into(), "c".into()];
        let covered = BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1u32]))]);
        let _ = publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &identity,
            scope: WitnessScope::Full,
            selectors: &witness_selectors,
            statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: &[Some(12_000_000_000), Some(20)],
            covered_lines: &covered,
            complete: true,
            jobs: 1,
        })
        .unwrap();
        let tight = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 5.0)],
            ..kiss::GateConfig::default()
        };
        assert!(
            try_load_rust_coverage_from_witness(tmp.path(), &identity, &enumerator, &tight)
                .is_none()
        );
    }
}
