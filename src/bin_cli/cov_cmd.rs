use crate::analyze;
use crate::analyze::line_coverage::RuntimeCoverageSnapshot;
use crate::analyze::line_coverage::compute_line_coverage_records;
use crate::analyze::{build_focus_filter, gather_files};
use crate::bin_cli::util::{merge_check_ignore_prefixes, validate_paths};
use crate::test_runner::check_line_coverage::{
    RequiredCoverageLanguages, ensure_check_runtime_coverage, load_check_runtime_coverage,
    repository_root_for_universe,
};
use kiss::Language;
use kiss::cli_output::{print_final_status, print_no_files_message, print_violations};
use std::path::Path;

pub struct CovCommandArgs<'a> {
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub py_config: &'a kiss::Config,
    pub rs_config: &'a kiss::Config,
    pub gate_config: &'a kiss::GateConfig,
    pub bypass_gate: bool,
    pub ignore: &'a [String],
    pub timing: bool,
    pub jobs: usize,
}

fn load_or_refresh_snapshot(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
    jobs: usize,
) -> Result<RuntimeCoverageSnapshot, i32> {
    match load_check_runtime_coverage(repo_root, required, ignore) {
        Ok(snapshot) => Ok(snapshot),
        Err(_) => {
            if let Err(err) = ensure_check_runtime_coverage(repo_root, required, ignore, jobs) {
                eprintln!("{err}");
                return Err(1);
            }
            load_check_runtime_coverage(repo_root, required, ignore).map_err(|err| {
                eprintln!("{err}");
                1
            })
        }
    }
}

fn finish_with_coverage_violations(
    records: &[analyze::line_coverage::LineCoverageRecord],
    focus: &analyze::FocusFilter,
    bypass_gate: bool,
) -> i32 {
    let viols = analyze::collect_line_coverage_viols(records, focus, bypass_gate);
    print_violations(&viols);
    let has_violations = !viols.is_empty();
    print_final_status(has_violations);
    i32::from(has_violations)
}

pub fn run_cov_command(args: &CovCommandArgs<'_>) -> i32 {
    let _ = (args.py_config, args.rs_config);
    let ignore = merge_check_ignore_prefixes(args.ignore);
    validate_paths(args.paths);
    let universe = &args.paths[0];
    let focus_paths = if args.paths.len() > 1 {
        &args.paths[1..]
    } else {
        args.paths
    };
    let universe_root = Path::new(universe);
    let (py_files, rs_files) = gather_files(universe_root, args.lang_filter, &ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(args.lang_filter, universe_root);
        return 0;
    }
    let threshold = args.gate_config.test_coverage_threshold;
    if threshold == 0 && !args.bypass_gate {
        print_final_status(false);
        return 0;
    }
    let t0 = std::time::Instant::now();
    let repo_root = repository_root_for_universe(universe_root);
    let required = RequiredCoverageLanguages {
        python: !py_files.is_empty(),
        rust: !rs_files.is_empty(),
    };
    let snapshot = match load_or_refresh_snapshot(&repo_root, required, &ignore, args.jobs) {
        Ok(snapshot) => snapshot,
        Err(code) => return code,
    };
    if args.timing {
        eprintln!(
            "TIMING:coverage_snapshot_load_or_refresh_ms:{}",
            t0.elapsed().as_millis()
        );
    }
    let records = compute_line_coverage_records(&repo_root, &py_files, &rs_files, &snapshot);
    let focus = build_focus_filter(focus_paths, universe, args.lang_filter, &ignore);
    if !args.bypass_gate
        && threshold > 0
        && let Some(result) = analyze::evaluate_line_gate(&records, &focus, threshold)
    {
        return i32::from(!result.success);
    }
    finish_with_coverage_violations(&records, &focus, args.bypass_gate)
}

