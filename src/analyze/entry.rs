use std::path::Path;

use crate::analyze::focus::{FocusFilter, build_focus_filter, gather_files};
use crate::analyze::options::{AnalyzeOptions, AnalyzeResult};
use crate::analyze::params::RunAnalyzeUncached;
use crate::analyze::pipeline::run_analyze_uncached;
use kiss::cli_output::print_no_files_message;

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
) -> Option<AnalyzeResult> {
    if opts.show_timing || opts.suppress_final_status {
        return None;
    }
    crate::analyze_cache::try_run_cached_all(opts, py_files, rs_files, focus)
        .map(|ok| AnalyzeResult { success: ok })
}

pub fn run_analyze(opts: &AnalyzeOptions<'_>) -> bool {
    run_analyze_with_result(opts).success
}

pub fn run_analyze_with_result(opts: &AnalyzeOptions<'_>) -> AnalyzeResult {
    let t0 = std::time::Instant::now();
    let universe_root = Path::new(opts.universe);
    let (py_files, rs_files) = gather_files(universe_root, opts.lang_filter, opts.ignore_prefixes);
    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(opts.lang_filter, universe_root);
        return AnalyzeResult { success: true };
    }
    let focus = focus_filter_for_opts(opts);
    if let Some(hit) = try_cache_hit(opts, &py_files, &rs_files, &focus) {
        return hit;
    }
    let t1 = std::time::Instant::now();
    run_analyze_uncached(RunAnalyzeUncached {
        opts,
        py_files: &py_files,
        rs_files: &rs_files,
        focus: &focus,
        t0,
        t1,
    })
}

#[cfg(test)]
mod entry_touch {
    use super::*;

    fn sample_opts<'a>(
        universe: &'a str,
        focus: &'a [String],
        py_cfg: &'a kiss::Config,
        rs_cfg: &'a kiss::Config,
        gate: &'a kiss::GateConfig,
        show_timing: bool,
        suppress_final_status: bool,
    ) -> AnalyzeOptions<'a> {
        AnalyzeOptions {
            universe,
            focus_paths: focus,
            py_config: py_cfg,
            rs_config: rs_cfg,
            lang_filter: None,
            bypass_gate: false,
            gate_config: gate,
            ignore_prefixes: &[],
            show_timing,
            suppress_final_status,
        }
    }

    #[test]
    fn test_focus_filter_for_universe_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let universe = tmp.path().to_str().unwrap().to_string();
        let focus = vec![universe.clone()];
        let py_cfg = kiss::Config::python_defaults();
        let rs_cfg = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let opts = sample_opts(&universe, &focus, &py_cfg, &rs_cfg, &gate, false, false);
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
        let opts = sample_opts(&universe, &focus, &py_cfg, &rs_cfg, &gate, true, false);
        let focus_filter = FocusFilter::unrestricted();
        assert!(try_cache_hit(&opts, &[], &[], &focus_filter).is_none());
    }

    #[test]
    fn test_try_cache_hit_skips_cache_when_suppress_final_status() {
        let tmp = tempfile::TempDir::new().unwrap();
        let universe = tmp.path().to_str().unwrap().to_string();
        let focus = vec![universe.clone()];
        let py_cfg = kiss::Config::python_defaults();
        let rs_cfg = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let opts = sample_opts(&universe, &focus, &py_cfg, &rs_cfg, &gate, false, true);
        let focus_filter = FocusFilter::unrestricted();
        assert!(try_cache_hit(&opts, &[], &[], &focus_filter).is_none());
    }

    #[test]
    fn test_run_analyze_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let universe = tmp.path().to_str().unwrap().to_string();
        let focus = vec![universe.clone()];
        let py_cfg = kiss::Config::python_defaults();
        let rs_cfg = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let opts = sample_opts(&universe, &focus, &py_cfg, &rs_cfg, &gate, false, true);
        let result = run_analyze_with_result(&opts);
        assert!(result.success);
    }
}
