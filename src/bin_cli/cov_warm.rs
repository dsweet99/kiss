use std::path::Path;

use kiss::{GateConfig, Language};

use crate::analyze::cov_coverable_cache::{load_or_build_coverable_denoms, CovCoverableKey};
use crate::analyze::cov_records_cache::{
    store_cov_records, try_load_cov_records, CovRecordsCacheKey,
};
use crate::analyze::line_coverage::records_from_denoms;
use crate::bin_cli::util::merge_check_ignore_prefixes;
use crate::test_runner::check_line_coverage::{
    load_check_runtime_coverage, repository_root_for_universe, RequiredCoverageLanguages,
};

#[allow(dead_code)]
pub(crate) fn warm_cov_caches_after_tests(
    universe_root: &Path,
    lang_filter: Option<Language>,
    ignore_user: &[String],
    gate: &GateConfig,
    pytest_args: &[String],
) {
    let started = std::time::Instant::now();
    warm_cov_caches_after_tests_inner(universe_root, lang_filter, ignore_user, gate, pytest_args);
    crate::test_runner::emit_stage_time("cov_score_warm", started.elapsed());
}

fn warm_cov_caches_after_tests_inner(
    universe_root: &Path,
    lang_filter: Option<Language>,
    ignore_user: &[String],
    gate: &GateConfig,
    pytest_args: &[String],
) {
    crate::test_runner::python_coverage_index::clear_python_generation_warm_memo();
    let ignore = merge_check_ignore_prefixes(ignore_user);
    if gate.test_coverage_threshold == 0 && gate.unit_test_time_gate_disabled() {
        return;
    }
    let Some(files) =
        crate::bin_cli::cov_cmd_cache::gather_cov_files(universe_root, lang_filter, &ignore)
    else {
        return;
    };
    let repo_root = repository_root_for_universe(universe_root);
    let required = RequiredCoverageLanguages {
        python: !files.py_files.is_empty(),
        rust: !files.rs_files.is_empty(),
    };
    let cache_key = CovRecordsCacheKey {
        repo_root: &repo_root,
        py_files: &files.py_files,
        rs_files: &files.rs_files,
        required,
        threshold: gate.test_coverage_threshold,
        bypass_gate: false,
        ignore: &ignore,
        lang_filter: lang_filter.map(|lang| match lang {
            Language::Python => "python",
            Language::Rust => "rust",
        }),
        pytest_args,
    };
    if try_load_cov_records(&cache_key).is_some() {
        return;
    }
    let Ok(snapshot) =
        load_check_runtime_coverage(&repo_root, required, &ignore, gate, pytest_args)
    else {
        return;
    };
    let facts_key = CovCoverableKey {
        repo_root: &repo_root,
        py_files: &files.py_files,
        rs_files: &files.rs_files,
        ignore: &ignore,
        lang_filter: cache_key.lang_filter,
    };
    let records = match load_or_build_coverable_denoms(&facts_key) {
        Ok(denoms) => records_from_denoms(&repo_root, &denoms, &snapshot),
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };
    store_cov_records(&cache_key, &records);
}

#[cfg(test)]
#[path = "cov_warm_test.rs"]
mod tests;
