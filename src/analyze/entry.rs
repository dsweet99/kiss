use std::collections::HashSet;
use std::path::Path;

use crate::analyze::focus::{build_focus_set, gather_files};
use crate::analyze::options::{AnalyzeOptions, AnalyzeResult};
use crate::analyze::params::RunAnalyzeUncached;
use crate::analyze::pipeline::run_analyze_uncached;
use kiss::cli_output::print_no_files_message;

fn empty_repo_metrics() -> kiss::GlobalMetrics {
    kiss::GlobalMetrics::default()
}

fn focus_set_for_opts(
    opts: &AnalyzeOptions<'_>,
    py_files: &[std::path::PathBuf],
    rs_files: &[std::path::PathBuf],
) -> HashSet<std::path::PathBuf> {
    if opts.focus_paths.len() == 1 && opts.focus_paths[0] == opts.universe {
        let mut set = HashSet::with_capacity(py_files.len() + rs_files.len());
        set.extend(py_files.iter().cloned());
        set.extend(rs_files.iter().cloned());
        set
    } else {
        build_focus_set(opts.focus_paths, opts.lang_filter, opts.ignore_prefixes)
    }
}

fn try_cache_hit(
    opts: &AnalyzeOptions<'_>,
    py_files: &[std::path::PathBuf],
    rs_files: &[std::path::PathBuf],
    focus_set: &HashSet<std::path::PathBuf>,
) -> Option<AnalyzeResult> {
    if !(opts.bypass_gate && !opts.show_timing && !opts.suppress_final_status) {
        return None;
    }
    crate::analyze_cache::try_run_cached_all(opts, py_files, rs_files, focus_set)
        .map(|ok| AnalyzeResult {
            success: ok,
            metrics: None,
        })
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
    let focus_set = focus_set_for_opts(opts, &py_files, &rs_files);
    if let Some(hit) = try_cache_hit(opts, &py_files, &rs_files, &focus_set) {
        return hit;
    }
    let t1 = std::time::Instant::now();
    run_analyze_uncached(RunAnalyzeUncached {
        opts,
        py_files: &py_files,
        rs_files: &rs_files,
        focus_set: &focus_set,
        t0,
        t1,
    })
}

#[cfg(test)]
mod entry_touch {
    use super::empty_repo_metrics;

    #[test]
    fn empty_repo_matches_default_metrics() {
        assert_eq!(empty_repo_metrics(), kiss::GlobalMetrics::default());
    }
}
