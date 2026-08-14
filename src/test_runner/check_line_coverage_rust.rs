//! Rust runtime coverage loading for `kiss cov`.

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
    if let Some(cov) = try_load_rust_coverage_from_witness(repo_root, &identity, &selectors, gate) {
        return Ok(cov);
    }
    // Fail closed: never accept aggregate/population with expected_selectors = None
    // when proving readiness for the planned universe (plan invariant 7).
    if let Some(snapshot) = rust_llvm_cov_runner::load_current_check_aggregate_snapshot(
        &cache_root,
        repo_root,
        &identity,
        Some(&selectors),
    ) {
        return Ok(backend_from_lines(snapshot.identity, snapshot.covered_lines));
    }
    if let Some(snapshot) = rust_llvm_cov_runner::load_current_generation_coverage_snapshot(
        &cache_root,
        repo_root,
        &identity,
        Some(&selectors),
    ) {
        return Ok(backend_from_lines(
            snapshot.identity,
            remap_rust_covered_lines(repo_root, snapshot.covered_lines)?,
        ));
    }
    // Witness Accept + reusable prior aggregate (same selection context): coverage is
    // sufficient without rebuilding the executable index (plan invariant 6).
    if let Some(cov) = try_load_rust_coverage_from_witness_prior(
        repo_root,
        &cache_root,
        &identity,
        &selectors,
        gate,
    ) {
        return Ok(cov);
    }
    Err(coverage_error(
        "Rust",
        "missing, stale/incompatible, incomplete, or malformed population",
    ))
}

fn rust_selectors_for_coverage_load(
    repo_root: &Path,
    ignore: &[String],
) -> Result<Vec<String>, RuntimeCoverageLoadError> {
    // Prefer the warm workspace selector cache (same plan as `kiss test`) so
    // post-test coverage does not re-walk/parse the whole Rust tree.
    if let Some((_, rust_selectors, _)) =
        crate::test_runner::workspace_selector_cache::load_cached_workspace_selectors(
            repo_root, ignore,
        )
    {
        return Ok(rust_selectors);
    }
    crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
        .map_err(|err| coverage_error("Rust", &format!("selector discovery failed ({err})")))
}

pub(super) fn try_load_rust_coverage_from_witness(
    repo_root: &Path,
    identity: &rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selectors: &[String],
    gate: &kiss::GateConfig,
) -> Option<BackendCoverage> {
    let mut witness = try_load_rust_execution_witness(repo_root).ok()?;
    if witness.covered_lines.is_empty() {
        return None;
    }
    // Silent Accept (do not print PASS lines or rebuild report-id maps).
    if !rust_witness_accepts_planned(repo_root, &mut witness, identity, selectors, gate) {
        return None;
    }
    let covered: BTreeMap<String, BTreeSet<u32>> = witness
        .covered_lines
        .into_iter()
        .map(|(path, lines)| (path, lines.into_iter().collect()))
        .collect();
    Some(backend_from_lines(witness.generation_id, covered))
}

fn rust_witness_accepts_planned(
    _repo_root: &Path,
    witness: &mut crate::test_runner::lang_iface::ExecutionWitness,
    identity: &rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selectors: &[String],
    gate: &kiss::GateConfig,
) -> bool {
    use crate::test_runner::execution_witness::rust_identity_digest_from_batch;
    use crate::test_runner::lang_iface::{
        AcceptDecision, AcceptMode, accept_witness, reclassify_statuses_with_gate,
    };
    let current = rust_identity_digest_from_batch(identity);
    if witness.identity_digest != current {
        return false;
    }
    witness.statuses = reclassify_statuses_with_gate(
        &witness.selectors,
        &witness.statuses,
        &witness.durations_ns,
        gate,
    );
    let mut planned = selectors.to_vec();
    planned.sort();
    planned.dedup();
    let mode = if planned == witness.selectors {
        AcceptMode::All
    } else {
        AcceptMode::Subset
    };
    accept_witness(mode, &planned, &current, witness) == AcceptDecision::Accept
}

pub(super) fn try_load_rust_coverage_from_witness_prior(
    repo_root: &Path,
    cache_root: &Path,
    identity: &rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selectors: &[String],
    gate: &kiss::GateConfig,
) -> Option<BackendCoverage> {
    let mut witness = try_load_rust_execution_witness(repo_root).ok()?;
    if !rust_witness_accepts_planned(repo_root, &mut witness, identity, selectors, gate) {
        return None;
    }
    // Load prior against the Full witness universe (not the possibly ignore-filtered
    // cov planned set). `load_reusable_prior_check_aggregate` requires exact
    // selector equality with the on-disk aggregate.
    let prior = rust_llvm_cov_runner::load_reusable_prior_check_aggregate(
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
    let mut covered_lines = BTreeMap::<String, BTreeSet<u32>>::new();
    for (file, lines) in covered {
        let rel = rust_repo_relative_coverage_file(repo_root, &file)
            .ok_or_else(|| coverage_error("Rust", "malformed out-of-repository path"))?;
        covered_lines.entry(rel).or_default().extend(lines);
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
        let mapped = remap_rust_covered_lines(
            repo,
            BTreeMap::from([(abs, BTreeSet::from([1u32]))]),
        )
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
        let identity = rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "g".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: Default::default(),
        };
        let selectors = vec!["a".into()];
        assert!(
            try_load_rust_coverage_from_witness(tmp.path(), &identity, &selectors, &kiss::GateConfig::default()).is_none()
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
        let identity = rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "g".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: Default::default(),
        };
        let selectors = vec!["a".into(), "b".into()];
        let covered = BTreeMap::from([(
            "src/lib.rs".into(),
            BTreeSet::from([1u32, 2u32]),
        )]);
        let _ = publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &identity,
            scope: WitnessScope::Full,
            selectors: &selectors,
            statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: &[Some(10), Some(20)],
            covered_lines: &covered,
            complete: true,
        })
        .unwrap();
        let loaded =
            try_load_rust_coverage_from_witness(tmp.path(), &identity, &selectors, &kiss::GateConfig::default()).unwrap();
        assert!(loaded.covered_lines.contains_key("src/lib.rs"));
        // Empty covered_lines → None even if witness would accept.
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
        })
        .unwrap();
        assert!(
            try_load_rust_coverage_from_witness(tmp.path(), &identity, &selectors, &kiss::GateConfig::default()).is_none()
        );
    }
}
