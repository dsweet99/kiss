use std::path::{Path, PathBuf};
use std::time::Instant;

use kiss::Language;

use crate::analyze;
use crate::analyze::cov_coverable_cache::{CovCoverableKey, load_or_build_coverable_denoms};
use crate::analyze::cov_records_cache::{
    CovRecordsCacheKey, store_cov_records,
};
use crate::analyze::gather_files;
use crate::analyze::line_coverage::{RuntimeCoverageSnapshot, records_from_denoms};
use crate::test_runner::check_line_coverage::repository_root_for_universe;

pub(crate) struct CovFileSets {
    pub(crate) py_files: Vec<PathBuf>,
    pub(crate) rs_files: Vec<PathBuf>,
}

pub(crate) fn gather_cov_files(
    universe_root: &Path,
    lang_filter: Option<Language>,
    ignore: &[String],
) -> Option<CovFileSets> {
    let repo_root = repository_root_for_universe(universe_root);
    let list_key = crate::analyze::cov_file_list_cache::CovFileListKey {
        repo_root: &repo_root,
        lang_filter,
        ignore,
    };
    let (py_files, mut rs_files) = if let Some(cached) =
        crate::analyze::cov_file_list_cache::try_load_cov_file_list(&list_key)
    {
        cached
    } else {
        let (py_files, rs_files) = gather_files(universe_root, lang_filter, ignore);
        if !py_files.is_empty() || !rs_files.is_empty() {
            crate::analyze::cov_file_list_cache::store_cov_file_list(
                &list_key, &py_files, &rs_files,
            );
        }
        (py_files, rs_files)
    };
    rs_files =
        super::cov_workspace_files::filter_root_workspace_rust_cov_files(&repo_root, rs_files);
    if py_files.is_empty() && rs_files.is_empty() {
        None
    } else {
        Some(CovFileSets { py_files, rs_files })
    }
}

pub(crate) fn lang_filter_cache_label(lang_filter: Option<Language>) -> Option<&'static str> {
    lang_filter.map(|lang| match lang {
        Language::Python => "python",
        Language::Rust => "rust",
    })
}

pub(crate) fn compute_and_store_records(
    cache_key: &CovRecordsCacheKey<'_>,
    repo_root: &Path,
    files: &CovFileSets,
    snapshot: &RuntimeCoverageSnapshot,
    timing: bool,
    t0: Instant,
) -> Result<Vec<analyze::line_coverage::LineCoverageRecord>, kiss::RoleBuildError> {
    if timing {
        eprintln!(
            "TIMING:coverage_snapshot_load_or_refresh_ms:{}",
            t0.elapsed().as_millis()
        );
    }
    let t_records = Instant::now();
    let facts_key = CovCoverableKey {
        repo_root,
        py_files: &files.py_files,
        rs_files: &files.rs_files,
        ignore: cache_key.ignore,
        lang_filter: cache_key.lang_filter,
    };
    let denoms = load_or_build_coverable_denoms(&facts_key)?;
    let records = records_from_denoms(repo_root, &denoms, snapshot);
    if timing {
        eprintln!(
            "TIMING:coverage_records_compute_ms:{}",
            t_records.elapsed().as_millis()
        );
    }
    store_cov_records(cache_key, &records);
    Ok(records)
}
