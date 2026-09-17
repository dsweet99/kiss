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

#[cfg(test)]
pub(crate) fn set_pairs_for_tests(repo_root: &Path, pairs: Vec<DurationPair>) {
    RUST_DURATION_PAIRS_MEMO.with(|memo| {
        *memo.borrow_mut() = Some((repo_root.to_path_buf(), pairs));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_durations_memo_and_ignore_filtering() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();

        clear_rust_duration_pairs_memo();
        assert!(load_rust_duration_pairs(path).is_none());

        RUST_DURATION_PAIRS_MEMO.with(|memo| {
            *memo.borrow_mut() = Some((
                path.to_path_buf(),
                vec![
                    ("tests/ignored/t.rs::test_x".to_string(), Duration::from_secs(10)),
                    ("tests/kept/t.rs::test_y".to_string(), Duration::from_secs(2)),
                ],
            ));
        });

        let pairs = load_rust_duration_pairs(path).unwrap();
        assert_eq!(pairs.len(), 2);

        let max = load_rust_population_max_duration(path, &["tests/ignored".to_string()]);
        assert_eq!(max, Some(Duration::from_secs(2)));

        let none = load_rust_population_max_duration(path, &["tests/".to_string()]);
        assert_eq!(none, None);

        clear_rust_duration_pairs_memo();
        RUST_DURATION_PAIRS_MEMO.with(|memo| {
            assert!(memo.borrow().is_none());
        });
    }

    #[test]
    fn rust_logical_mod_tests_are_not_ignored_as_tests_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        clear_rust_duration_pairs_memo();
        set_pairs_for_tests(
            path,
            vec![("tests::unit_ok".to_string(), Duration::from_secs(2))],
        );
        assert_eq!(
            load_rust_population_max_duration(path, &["tests".to_string()]),
            Some(Duration::from_secs(2)),
            "rust logical tests::unit_ok is not the tests/ directory"
        );
        clear_rust_duration_pairs_memo();
    }

    #[test]
    fn load_rust_duration_pairs_from_witness_absent_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "digest".to_string(),
            generation_fingerprint: "fingerprint".to_string(),
            selection_context_fingerprint: "sel".to_string(),
            ordinary_source_digests: std::collections::BTreeMap::new(),
        };
        assert!(load_rust_duration_pairs_from_witness(tmp.path(), &identity).is_none());
    }

    #[test]
    fn load_rust_duration_pairs_from_witness_present_returns_pairs() {
        use crate::test_runner::execution_witness::{WitnessScope, WitnessStatus};
        use crate::test_runner::lang_rust::{
            PublishRustWitness, publish_rust_execution_witness,
        };

        let tmp = tempfile::tempdir().unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "digest".to_string(),
            generation_fingerprint: "fingerprint".to_string(),
            selection_context_fingerprint: "sel".to_string(),
            ordinary_source_digests: std::collections::BTreeMap::new(),
        };

        let selectors = vec!["tests/a.rs::test_1".to_string()];
        let statuses = vec![WitnessStatus::Passed];
        let durations_ns = vec![Some(2_000_000_000)];
        let covered_lines = std::collections::BTreeMap::new();

        publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &identity,
            scope: WitnessScope::Full,
            selectors: &selectors,
            statuses: &statuses,
            durations_ns: &durations_ns,
            covered_lines: &covered_lines,
            complete: true,
            jobs: 1,
        })
        .unwrap();

        let pairs = load_rust_duration_pairs_from_witness(tmp.path(), &identity).unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "tests/a.rs::test_1");
        assert_eq!(pairs[0].1, Duration::from_secs(2));
    }
}

