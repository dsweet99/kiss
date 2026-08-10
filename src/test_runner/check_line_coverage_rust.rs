//! Rust runtime coverage loading for `kiss cov`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{BackendCoverage, RuntimeCoverageLoadError, coverage_error};
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity,
    repo_relative_coverage_file as rust_repo_relative_coverage_file, rust_coverage_cache_root,
};

pub(crate) fn load_rust_runtime_coverage(
    repo_root: &Path,
    ignore: &[String],
) -> Result<BackendCoverage, RuntimeCoverageLoadError> {
    let identity = current_rust_coverage_batch_identity(repo_root, &[]).map_err(|err| {
        coverage_error("Rust", &format!("stale/incompatible tool identity ({err})"))
    })?;
    let cache_root = rust_coverage_cache_root(repo_root);
    // Prefer aggregate hit without re-enumerating workspace selectors. Source digests in
    // `identity` already invalidate when test/source files change.
    if let Some(snapshot) = rust_llvm_cov_runner::load_current_check_aggregate_snapshot(
        &cache_root,
        repo_root,
        &identity,
        None,
    ) {
        return Ok(backend_from_lines(snapshot.identity, snapshot.covered_lines));
    }
    let selectors =
        crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
            .map_err(|err| coverage_error("Rust", &format!("selector discovery failed ({err})")))?;
    if let Some(snapshot) = rust_llvm_cov_runner::load_current_check_aggregate_snapshot(
        &cache_root,
        repo_root,
        &identity,
        Some(&selectors),
    ) {
        return Ok(backend_from_lines(snapshot.identity, snapshot.covered_lines));
    }
    let snapshot = rust_llvm_cov_runner::load_current_generation_coverage_snapshot(
        &cache_root,
        repo_root,
        &identity,
        Some(&selectors),
    )
    .ok_or_else(|| {
        coverage_error(
            "Rust",
            "missing, stale/incompatible, incomplete, or malformed population",
        )
    })?;
    Ok(backend_from_lines(
        snapshot.identity,
        remap_rust_covered_lines(repo_root, snapshot.covered_lines)?,
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
}
