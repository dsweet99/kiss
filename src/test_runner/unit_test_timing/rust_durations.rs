use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::test_runner::rust_coverage_index::{
    resolved_rust_batch_request_parts, rust_coverage_cache_root,
};

use super::selector_matches_ignore_prefix;

type DurationPair = (String, Duration);
type DurationPairsMemo = Option<(PathBuf, Vec<DurationPair>)>;

thread_local! {
    static RUST_DURATION_PAIRS_MEMO: RefCell<DurationPairsMemo> = const { RefCell::new(None) };
}

pub(crate) fn clear_rust_duration_pairs_memo() {
    RUST_DURATION_PAIRS_MEMO.with(|memo| {
        *memo.borrow_mut() = None;
    });
}

pub(crate) fn load_rust_population_max_duration(
    repo_root: &Path,
    ignore: &[String],
) -> Option<Duration> {
    let pairs = load_rust_duration_pairs(repo_root)?;
    let mut max = Duration::ZERO;
    let mut any = false;
    for (selector, duration) in pairs {
        if selector_matches_ignore_prefix(&selector, ignore) {
            continue;
        }
        any = true;
        max = max.max(duration);
    }
    any.then_some(max)
}

pub(super) fn load_rust_duration_pairs(repo_root: &Path) -> Option<Vec<DurationPair>> {
    let repo_key = repo_root.to_path_buf();
    if let Some(cached) = RUST_DURATION_PAIRS_MEMO.with(|memo| {
        memo.borrow()
            .as_ref()
            .filter(|(root, _)| root == &repo_key)
            .map(|(_, pairs)| pairs.clone())
    }) {
        return Some(cached);
    }
    let pairs = load_rust_duration_pairs_uncached(repo_root)?;
    RUST_DURATION_PAIRS_MEMO.with(|memo| {
        *memo.borrow_mut() = Some((repo_key, pairs.clone()));
    });
    Some(pairs)
}

fn load_rust_duration_pairs_uncached(repo_root: &Path) -> Option<Vec<DurationPair>> {
    let cache_root = rust_coverage_cache_root(repo_root);
    if let Some(pairs) =
        kiss::rust_llvm_cov_runner::try_load_sealed_population_durations(&cache_root, repo_root)
        && !pairs.is_empty()
    {
        return Some(pairs);
    }
    if let Some(sealed) = kiss::rust_llvm_cov_runner::try_source_matched_seal_identity(
        &cache_root,
        repo_root,
    ) && kiss::rust_llvm_cov_runner::current_population_manifest_matches_identity(
        &cache_root, &sealed,
    )
    .unwrap_or(false)
    {
        if let Some(pairs) = load_rust_duration_pairs_from_witness(repo_root, &sealed) {
            return Some(pairs);
        }
        if let Some(pairs) =
            load_rust_duration_pairs_via_batch(repo_root, &cache_root, Some(sealed))
        {
            return Some(pairs);
        }
    }
    load_rust_duration_pairs_via_batch(repo_root, &cache_root, None)
}

fn load_rust_duration_pairs_via_batch(
    repo_root: &Path,
    cache_root: &Path,
    sealed: Option<kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity>,
) -> Option<Vec<DurationPair>> {
    let (req, tools) = resolved_rust_batch_request_parts(repo_root, &[]).ok()?;
    let identity = match sealed {
        Some(sealed) => sealed,
        None => kiss::rust_llvm_cov_runner::batch_identity(&req, &tools).ok()?,
    };
    if let Some(pairs) = load_rust_duration_pairs_from_witness(repo_root, &identity) {
        return Some(pairs);
    }
    kiss::rust_llvm_cov_runner::load_current_population_durations(
        cache_root,
        repo_root,
        &identity,
        &req,
        &tools,
        None,
    )
    .filter(|pairs| !pairs.is_empty())
}

fn load_rust_duration_pairs_from_witness(
    repo_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
) -> Option<Vec<DurationPair>> {
    use crate::test_runner::execution_witness::{
        rust_identity_digest_from_batch, try_load_rust_execution_witness,
    };
    use crate::test_runner::lang_iface::identity_covers;
    let witness = try_load_rust_execution_witness(repo_root).ok()?;
    if !identity_covers(
        &witness.identity_digest,
        &rust_identity_digest_from_batch(identity),
    ) {
        return None;
    }
    if !witness.complete
        || witness.selectors.is_empty()
        || witness.durations_ns.len() != witness.selectors.len()
    {
        return None;
    }
    witness
        .selectors
        .iter()
        .zip(witness.durations_ns.iter())
        .map(|(selector, &ns)| Some((selector.clone(), Duration::from_nanos(ns?))))
        .collect()
}
