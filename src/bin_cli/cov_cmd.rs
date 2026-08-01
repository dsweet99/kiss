use crate::analyze;
use crate::analyze::cov_records_cache::{CovRecordsCacheKey, store_cov_records, try_load_cov_records};
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
use std::path::{Path, PathBuf};
use std::time::Instant;

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

fn evaluate_records(
    records: &[analyze::line_coverage::LineCoverageRecord],
    focus: &analyze::FocusFilter,
    threshold: usize,
    scope: kiss::TestCoverageScope,
    bypass_gate: bool,
) -> i32 {
    if !bypass_gate
        && threshold > 0
        && let Some(result) = analyze::evaluate_line_gate(records, focus, threshold, scope)
    {
        return i32::from(!result.success);
    }
    finish_with_coverage_violations(records, focus, bypass_gate)
}

struct CovFileSets {
    py_files: Vec<PathBuf>,
    rs_files: Vec<PathBuf>,
}

fn gather_cov_files(
    universe_root: &Path,
    lang_filter: Option<Language>,
    ignore: &[String],
) -> Option<CovFileSets> {
    let (py_files, rs_files) = gather_files(universe_root, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        None
    } else {
        Some(CovFileSets { py_files, rs_files })
    }
}

fn lang_filter_cache_label(lang_filter: Option<Language>) -> Option<&'static str> {
    lang_filter.map(|lang| match lang {
        Language::Python => "python",
        Language::Rust => "rust",
    })
}

fn compute_and_store_records(
    cache_key: &CovRecordsCacheKey<'_>,
    repo_root: &Path,
    files: &CovFileSets,
    snapshot: &RuntimeCoverageSnapshot,
    timing: bool,
    t0: Instant,
) -> Vec<analyze::line_coverage::LineCoverageRecord> {
    if timing {
        eprintln!(
            "TIMING:coverage_snapshot_load_or_refresh_ms:{}",
            t0.elapsed().as_millis()
        );
    }
    let t_records = Instant::now();
    let records = compute_line_coverage_records(
        repo_root,
        &files.py_files,
        &files.rs_files,
        snapshot,
    );
    if timing {
        eprintln!(
            "TIMING:coverage_records_compute_ms:{}",
            t_records.elapsed().as_millis()
        );
    }
    store_cov_records(cache_key, &records);
    records
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
    let t_gather = Instant::now();
    let Some(files) = gather_cov_files(universe_root, args.lang_filter, &ignore) else {
        print_no_files_message(args.lang_filter, universe_root);
        return 0;
    };
    if args.timing {
        eprintln!(
            "TIMING:coverage_gather_files_ms:{}",
            t_gather.elapsed().as_millis()
        );
    }
    let threshold = args.gate_config.test_coverage_threshold;
    if threshold == 0 && !args.bypass_gate {
        print_final_status(false);
        return 0;
    }
    evaluate_gathered_cov(EvaluateGatheredCov {
        args,
        ignore: &ignore,
        focus_paths,
        universe,
        universe_root,
        files: &files,
        threshold,
    })
}

struct EvaluateGatheredCov<'a> {
    args: &'a CovCommandArgs<'a>,
    ignore: &'a [String],
    focus_paths: &'a [String],
    universe: &'a str,
    universe_root: &'a Path,
    files: &'a CovFileSets,
    threshold: usize,
}

fn evaluate_gathered_cov(p: EvaluateGatheredCov<'_>) -> i32 {
    let scope = p.args.gate_config.test_coverage_scope;
    let repo_root = repository_root_for_universe(p.universe_root);
    let required = RequiredCoverageLanguages {
        python: !p.files.py_files.is_empty(),
        rust: !p.files.rs_files.is_empty(),
    };
    let cache_key = CovRecordsCacheKey {
        repo_root: &repo_root,
        py_files: &p.files.py_files,
        rs_files: &p.files.rs_files,
        required,
        threshold: p.threshold,
        bypass_gate: p.args.bypass_gate,
        ignore: p.ignore,
        lang_filter: lang_filter_cache_label(p.args.lang_filter),
    };
    let focus = build_focus_filter(p.focus_paths, p.universe, p.args.lang_filter, p.ignore);
    let t0 = Instant::now();
    if let Some(code) = try_cov_records_fast_path(&cache_key, &focus, p.threshold, scope, p.args) {
        return code;
    }
    let snapshot = match load_or_refresh_snapshot(&repo_root, required, p.ignore, p.args.jobs) {
        Ok(snapshot) => snapshot,
        Err(code) => return code,
    };
    let records =
        compute_and_store_records(&cache_key, &repo_root, p.files, &snapshot, p.args.timing, t0);
    evaluate_records(&records, &focus, p.threshold, scope, p.args.bypass_gate)
}

/// Hit `cov_records_cache` when present under `./.kiss`.
fn try_cov_records_fast_path(
    cache_key: &CovRecordsCacheKey<'_>,
    focus: &analyze::FocusFilter,
    threshold: usize,
    scope: kiss::TestCoverageScope,
    args: &CovCommandArgs<'_>,
) -> Option<i32> {
    let t0 = Instant::now();
    let records = try_load_cov_records(cache_key)?;
    Some(finish_records_cache_hit(
        &records, focus, threshold, scope, args, t0,
    ))
}

fn finish_records_cache_hit(
    records: &[analyze::line_coverage::LineCoverageRecord],
    focus: &analyze::FocusFilter,
    threshold: usize,
    scope: kiss::TestCoverageScope,
    args: &CovCommandArgs<'_>,
    t0: Instant,
) -> i32 {
    if args.timing {
        eprintln!(
            "TIMING:coverage_records_cache_hit_ms:{}",
            t0.elapsed().as_millis()
        );
    }
    evaluate_records(records, focus, threshold, scope, args.bypass_gate)
}
