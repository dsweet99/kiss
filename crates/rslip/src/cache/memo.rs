use std::collections::BTreeMap;
use std::path::Path;

use super::{
    RslipCacheEntry, digest_recorded_path, is_non_digestable_coverage_path, load_rslip_cache_entry,
    test_module_path_from_nodeid,
};
use crate::LineCoverage;

pub(crate) type DigestMemo = std::collections::HashMap<String, Option<String>>;

pub(crate) fn digest_recorded_path_memo(
    source_root: &Path,
    recorded: &str,
    memo: &mut DigestMemo,
) -> Option<String> {
    if let Some(cached) = memo.get(recorded) {
        return cached.clone();
    }
    let digest = digest_recorded_path(source_root, recorded);
    memo.insert(recorded.to_string(), digest.clone());
    digest
}

pub(crate) fn entry_is_reusable_with_memo(
    entry: &RslipCacheEntry,
    source_root: &Path,
    memo: &mut DigestMemo,
) -> bool {
    if entry.status != rpytest_runner::TestStatus::Passed {
        return false;
    }
    if entry.coverage.files.is_empty() {
        return false;
    }
    let Some(expected) =
        covered_file_digests_with_memo(source_root, &entry.nodeid, &entry.coverage, memo)
    else {
        return false;
    };
    expected == entry.covered_digests
}

fn covered_file_digests_with_memo(
    source_root: &Path,
    nodeid: &str,
    coverage: &LineCoverage,
    memo: &mut DigestMemo,
) -> Option<BTreeMap<String, String>> {
    if coverage.files.is_empty() {
        return None;
    }
    let mut digests = BTreeMap::new();
    for recorded in coverage.files.keys() {
        if is_non_digestable_coverage_path(recorded) {
            continue;
        }
        let digest = digest_recorded_path_memo(source_root, recorded, memo)?;
        digests.insert(recorded.clone(), digest);
    }
    let module = test_module_path_from_nodeid(nodeid);
    if !module.is_empty()
        && !is_non_digestable_coverage_path(module)
        && let Some(digest) = digest_recorded_path_memo(source_root, module, memo)
    {
        digests.insert(module.to_string(), digest);
    }
    if digests.is_empty() {
        return Some(digests);
    }
    Some(digests)
}

pub(crate) fn load_reusable_rslip_cache_entry_with_memo(
    cache_root: &Path,
    fingerprint: &str,
    source_root: &Path,
    memo: &mut DigestMemo,
) -> Option<RslipCacheEntry> {
    let entry = load_rslip_cache_entry(cache_root, fingerprint)?;
    entry_is_reusable_with_memo(&entry, source_root, memo).then_some(entry)
}
