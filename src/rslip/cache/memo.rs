use std::collections::BTreeMap;
use std::path::Path;

use super::{
    RslipCacheEntry, digest_recorded_path, is_non_digestable_coverage_path, load_rslip_cache_entry,
    test_module_path_from_nodeid,
};
use crate::rslip::{LineCoverage, RslipRequest};

#[derive(Default)]
pub(crate) struct DigestMemo {
    files: std::collections::HashMap<String, Option<String>>,
    texts: std::collections::HashMap<String, Option<String>>,
}

impl DigestMemo {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn text(&mut self, source_root: &Path, recorded: &str) -> Option<String> {
        self.texts
            .entry(recorded.to_string())
            .or_insert_with(|| super::read_recorded_text(source_root, recorded))
            .clone()
    }
}

pub(crate) fn digest_recorded_path_memo(
    source_root: &Path,
    recorded: &str,
    memo: &mut DigestMemo,
) -> Option<String> {
    if let Some(cached) = memo.files.get(recorded) {
        return cached.clone();
    }
    let digest = digest_recorded_path(source_root, recorded);
    memo.files.insert(recorded.to_string(), digest.clone());
    digest
}

pub(crate) fn entry_is_reusable_with_memo(
    entry: &RslipCacheEntry,
    source_root: &Path,
    memo: &mut DigestMemo,
) -> bool {
    super::has_dependency_evidence(entry)
        && super::deps_still_hold(
            entry,
            covered_file_digests_with_memo(source_root, &entry.nodeid, &entry.coverage, memo)
                .as_ref(),
        )
}

pub(crate) fn covered_file_digests_with_memo(
    source_root: &Path,
    nodeid: &str,
    coverage: &LineCoverage,
    memo: &mut DigestMemo,
) -> Option<BTreeMap<String, String>> {
    if coverage.files.is_empty() {
        return super::module_digest_only(source_root, nodeid);
    }
    let module = test_module_path_from_nodeid(nodeid);
    let mut digests = BTreeMap::new();
    for (recorded, lines) in &coverage.files {
        if is_non_digestable_coverage_path(recorded) {
            continue;
        }
        let digest = if super::uses_statement_granularity(recorded, module) {
            super::statement_digest::covered_statements_digest(
                &memo.text(source_root, recorded)?,
                lines,
            )
        } else {
            digest_recorded_path_memo(source_root, recorded, memo)?
        };
        digests.insert(recorded.clone(), digest);
    }
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
    req: &RslipRequest,
    memo: &mut DigestMemo,
) -> Option<RslipCacheEntry> {
    let entry = load_rslip_cache_entry(req)?;
    entry_is_reusable_with_memo(&entry, &req.source_root, memo).then_some(entry)
}
