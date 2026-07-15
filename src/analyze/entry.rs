use std::path::Path;

use crate::analyze::focus::{FocusFilter, build_focus_filter, gather_files};
use crate::analyze::line_coverage::{
    LineCoverageRecord, RuntimeCoverageSnapshot, compute_line_coverage_records,
};
use crate::analyze::options::{AnalyzeOptions, AnalyzeResult, CoverageSource};
use crate::analyze::params::RunAnalyzeUncached;
use crate::analyze::pipeline::run_analyze_uncached;
use crate::test_runner::check_line_coverage::{
    CHECK_RUNTIME_REFRESH_ACTIVE_ENV, RequiredCoverageLanguages, ensure_check_runtime_coverage,
    load_check_runtime_coverage, repository_root_for_universe,
};
use kiss::cli_output::print_no_files_message;

fn empty_repo_metrics() -> kiss::GlobalMetrics {
    kiss::GlobalMetrics::default()
}

fn focus_filter_for_opts(opts: &AnalyzeOptions<'_>) -> FocusFilter {
    build_focus_filter(
        opts.focus_paths,
        opts.universe,
        opts.lang_filter,
        opts.ignore_prefixes,
    )
}

fn try_cache_hit(
    opts: &AnalyzeOptions<'_>,
    py_files: &[std::path::PathBuf],
    rs_files: &[std::path::PathBuf],
    focus: &FocusFilter,
    runtime_coverage_snapshot: Option<&RuntimeCoverageSnapshot>,
) -> Option<AnalyzeResult> {
    if opts.show_timing || opts.suppress_final_status {
        return None;
    }
    crate::analyze_cache::try_run_cached_all(
        opts,
        py_files,
        rs_files,
        focus,
        runtime_coverage_snapshot,
    )
    .map(|ok| AnalyzeResult {
        success: ok,
        metrics: None,
    })
}

fn runtime_coverage_needed(opts: &AnalyzeOptions<'_>) -> bool {
    opts.coverage_source == CoverageSource::RuntimeLine
        && (opts.gate_config.test_coverage_threshold > 0 || opts.bypass_gate)
        && std::env::var_os(CHECK_RUNTIME_REFRESH_ACTIVE_ENV).is_none()
}

fn load_runtime_coverage_for_check(
    opts: &AnalyzeOptions<'_>,
    py_files: &[std::path::PathBuf],
    rs_files: &[std::path::PathBuf],
) -> Result<Option<(RuntimeCoverageSnapshot, Vec<LineCoverageRecord>)>, AnalyzeResult> {
    if !runtime_coverage_needed(opts) {
        return Ok(None);
    }
    let repo_root = repository_root_for_universe(Path::new(opts.universe));
    let required = RequiredCoverageLanguages {
        python: !py_files.is_empty(),
        rust: !rs_files.is_empty(),
    };
    let snapshot = match load_check_runtime_coverage(&repo_root, required) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            if let Err(err) = ensure_check_runtime_coverage(
                &repo_root,
                required,
                opts.ignore_prefixes,
                opts.runtime_coverage_jobs,
            ) {
                eprintln!("{err}");
                return Err(AnalyzeResult {
                    success: false,
                    metrics: None,
                });
            }
            match load_check_runtime_coverage(&repo_root, required) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    eprintln!("{err}");
                    return Err(AnalyzeResult {
                        success: false,
                        metrics: None,
                    });
                }
            }
        }
    };
    let records = compute_line_coverage_records(&repo_root, py_files, rs_files, &snapshot);
    Ok(Some((snapshot, records)))
}

/// Run analysis and return a simple success/failure bool.
/// Use `run_analyze_with_result` if you need the computed metrics.
pub fn run_analyze(opts: &AnalyzeOptions<'_>) -> bool {
    run_analyze_with_result(opts).success
}

/// Run analysis and return detailed result including global metrics.
pub fn run_analyze_with_result(opts: &AnalyzeOptions<'_>) -> AnalyzeResult {
    let t0 = std::time::Instant::now();
    let universe_root = Path::new(opts.universe);
    let (py_files, rs_files) = gather_files(universe_root, opts.lang_filter, opts.ignore_prefixes);
    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(opts.lang_filter, universe_root);
        return AnalyzeResult {
            success: true,
            metrics: Some(empty_repo_metrics()),
        };
    }
    let focus = focus_filter_for_opts(opts);
    let runtime_coverage = match load_runtime_coverage_for_check(opts, &py_files, &rs_files) {
        Ok(runtime_coverage) => runtime_coverage,
        Err(result) => return result,
    };
    if let Some(hit) = try_cache_hit(
        opts,
        &py_files,
        &rs_files,
        &focus,
        runtime_coverage.as_ref().map(|(snapshot, _)| snapshot),
    ) {
        return hit;
    }
    let t1 = std::time::Instant::now();
    run_analyze_uncached(RunAnalyzeUncached {
        opts,
        py_files: &py_files,
        rs_files: &rs_files,
        focus: &focus,
        runtime_coverage_snapshot: runtime_coverage
            .as_ref()
            .map(|(snapshot, _)| snapshot.clone()),
        runtime_line_coverage: runtime_coverage.map(|(_, records)| records),
        t0,
        t1,
    })
}

#[cfg(test)]
mod entry_touch {
    use super::*;

    #[test]
    fn empty_repo_matches_default_metrics() {
        assert_eq!(empty_repo_metrics(), kiss::GlobalMetrics::default());
    }

    #[test]
    fn test_focus_filter_for_universe_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let universe = tmp.path().to_str().unwrap().to_string();
        let focus = vec![universe.clone()];
        let py_cfg = kiss::Config::python_defaults();
        let rs_cfg = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let opts = AnalyzeOptions {
            universe: &universe,
            focus_paths: &focus,
            py_config: &py_cfg,
            rs_config: &rs_cfg,
            lang_filter: None,
            bypass_gate: false,
            gate_config: &gate,
            ignore_prefixes: &[],
            show_timing: false,
            suppress_final_status: false,
            coverage_source: CoverageSource::StaticReferences,
            runtime_coverage_jobs: 1,
        };
        let filter = focus_filter_for_opts(&opts);
        assert!(!filter.is_active());
    }

    #[test]
    fn test_try_cache_hit_skips_cache_when_show_timing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let universe = tmp.path().to_str().unwrap().to_string();
        let focus = vec![universe.clone()];
        let py_cfg = kiss::Config::python_defaults();
        let rs_cfg = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let opts = AnalyzeOptions {
            universe: &universe,
            focus_paths: &focus,
            py_config: &py_cfg,
            rs_config: &rs_cfg,
            lang_filter: None,
            bypass_gate: false,
            gate_config: &gate,
            ignore_prefixes: &[],
            show_timing: true,
            suppress_final_status: false,
            coverage_source: CoverageSource::StaticReferences,
            runtime_coverage_jobs: 1,
        };
        let focus_filter = FocusFilter::unrestricted();
        assert!(try_cache_hit(&opts, &[], &[], &focus_filter, None).is_none());
    }

    #[test]
    fn test_try_cache_hit_skips_cache_when_suppress_final_status() {
        let tmp = tempfile::TempDir::new().unwrap();
        let universe = tmp.path().to_str().unwrap().to_string();
        let focus = vec![universe.clone()];
        let py_cfg = kiss::Config::python_defaults();
        let rs_cfg = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let opts = AnalyzeOptions {
            universe: &universe,
            focus_paths: &focus,
            py_config: &py_cfg,
            rs_config: &rs_cfg,
            lang_filter: None,
            bypass_gate: false,
            gate_config: &gate,
            ignore_prefixes: &[],
            show_timing: false,
            suppress_final_status: true,
            coverage_source: CoverageSource::StaticReferences,
            runtime_coverage_jobs: 1,
        };
        let focus_filter = FocusFilter::unrestricted();
        assert!(try_cache_hit(&opts, &[], &[], &focus_filter, None).is_none());
    }

    #[test]
    fn test_run_analyze_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let universe = tmp.path().to_str().unwrap().to_string();
        let focus = vec![universe.clone()];
        let py_cfg = kiss::Config::python_defaults();
        let rs_cfg = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let opts = AnalyzeOptions {
            universe: &universe,
            focus_paths: &focus,
            py_config: &py_cfg,
            rs_config: &rs_cfg,
            lang_filter: None,
            bypass_gate: false,
            gate_config: &gate,
            ignore_prefixes: &[],
            show_timing: false,
            suppress_final_status: true,
            coverage_source: CoverageSource::StaticReferences,
            runtime_coverage_jobs: 1,
        };
        let result = run_analyze_with_result(&opts);
        assert!(result.success);
    }
}
