use std::path::{Path, PathBuf};
use std::time::Instant;

use kiss::Language;

use crate::analyze;
use crate::analyze::cov_coverable_cache::{load_or_build_coverable_denoms, CovCoverableKey};
use crate::analyze::cov_records_cache::{store_cov_records, CovRecordsCacheKey};
use crate::analyze::gather_files;
use crate::analyze::line_coverage::{records_from_denoms, RuntimeCoverageSnapshot};
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
    let (mut py_files, mut rs_files) =
        load_or_gather_scoped_files(universe_root, &repo_root, lang_filter, ignore);
    rs_files =
        super::cov_workspace_files::filter_root_workspace_rust_cov_files(&repo_root, rs_files);
    retain_production_cov_files(&mut py_files, &mut rs_files);
    if py_files.is_empty() && rs_files.is_empty() {
        None
    } else {
        Some(CovFileSets { py_files, rs_files })
    }
}

fn load_or_gather_scoped_files(
    universe_root: &Path,
    repo_root: &Path,
    lang_filter: Option<Language>,
    ignore: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    if !universe_is_repo_root(universe_root, repo_root) {
        return gather_files(universe_root, lang_filter, ignore);
    }
    let list_key = crate::analyze::cov_file_list_cache::CovFileListKey {
        repo_root,
        lang_filter,
        ignore,
    };
    if let Some(cached) = crate::analyze::cov_file_list_cache::try_load_cov_file_list(&list_key) {
        return cached;
    }
    let gathered = gather_files(universe_root, lang_filter, ignore);
    if !gathered.0.is_empty() || !gathered.1.is_empty() {
        crate::analyze::cov_file_list_cache::store_cov_file_list(
            &list_key,
            &gathered.0,
            &gathered.1,
        );
    }
    gathered
}

fn universe_is_repo_root(universe_root: &Path, repo_root: &Path) -> bool {
    match (universe_root.canonicalize(), repo_root.canonicalize()) {
        (Ok(universe), Ok(repo)) => universe == repo,
        _ => false,
    }
}

fn retain_production_cov_files(py_files: &mut Vec<PathBuf>, rs_files: &mut Vec<PathBuf>) {
    py_files.retain(|path| !kiss::is_python_test_module_path(path));
    rs_files.retain(|path| !kiss::is_in_test_directory(path));
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
