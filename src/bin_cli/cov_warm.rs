//! Warm `kiss cov` derived caches after a successful `kiss test`.
//!
//! After tests leave the coverage population current, the first `kiss cov`
//! still had to gather sources and recompute per-file records (~seconds on
//! large repos). Publishing the same file-list + records caches `kiss cov`
//! uses makes the post-test cov path a cache hit.

use std::path::Path;

use kiss::Language;

use crate::analyze::cov_records_cache::{
    CovRecordsCacheKey, store_cov_records, try_load_cov_records,
};
use crate::analyze::line_coverage::compute_line_coverage_records;
use crate::analyze::gather_files;
use crate::bin_cli::util::merge_check_ignore_prefixes;
use crate::test_runner::check_line_coverage::{
    RequiredCoverageLanguages, load_check_runtime_coverage, repository_root_for_universe,
};

/// Ensure `.kiss/cov_file_list_cache.json` and `.kiss/cov_records_cache.json`
/// match the current population so the next `kiss cov` can stay warm.
pub(crate) fn warm_cov_caches_after_tests(
    universe_root: &Path,
    lang_filter: Option<Language>,
    ignore_user: &[String],
) {
    let ignore = merge_check_ignore_prefixes(ignore_user);
    let gate = kiss::GateConfig::load();
    if gate.test_coverage_threshold == 0 && gate.unit_test_time_gate_disabled() {
        return;
    }
    let repo_root = repository_root_for_universe(universe_root);
    let list_key = crate::analyze::cov_file_list_cache::CovFileListKey {
        repo_root: &repo_root,
        lang_filter,
        ignore: &ignore,
    };
    let (py_files, rs_files) =
        if let Some(cached) = crate::analyze::cov_file_list_cache::try_load_cov_file_list(&list_key)
        {
            cached
        } else {
            let (py_files, rs_files) = gather_files(universe_root, lang_filter, &ignore);
            if !py_files.is_empty() || !rs_files.is_empty() {
                crate::analyze::cov_file_list_cache::store_cov_file_list(
                    &list_key, &py_files, &rs_files,
                );
            }
            (py_files, rs_files)
        };
    if py_files.is_empty() && rs_files.is_empty() {
        return;
    }
    let required = RequiredCoverageLanguages {
        python: !py_files.is_empty(),
        rust: !rs_files.is_empty(),
    };
    let cache_key = CovRecordsCacheKey {
        repo_root: &repo_root,
        py_files: &py_files,
        rs_files: &rs_files,
        required,
        threshold: gate.test_coverage_threshold,
        bypass_gate: false,
        ignore: &ignore,
        lang_filter: lang_filter.map(|lang| match lang {
            Language::Python => "python",
            Language::Rust => "rust",
        }),
    };
    if try_load_cov_records(&cache_key).is_some() {
        return;
    }
    let Ok(snapshot) = load_check_runtime_coverage(&repo_root, required, &ignore) else {
        return;
    };
    let records = compute_line_coverage_records(&repo_root, &py_files, &rs_files, &snapshot);
    store_cov_records(&cache_key, &records);
}

#[cfg(test)]
#[path = "cov_warm_test.rs"]
mod tests;
