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
use crate::test_runner::unit_test_timing::{
    CovTimeGateOpts, RuntimeGateEval, TimingLangInclude, evaluate_cov_time_gate,
    runtime_gate_failure_lines,
};
#[cfg(test)]
use crate::test_runner::unit_test_timing::TimingCollectOpts;
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
    /// When false, evaluate from the current cache only (no population refresh).
    /// Used after a successful `kiss test` run that already ensured the cache.
    pub allow_refresh: bool,
}

fn load_or_refresh_snapshot(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
    jobs: usize,
    allow_refresh: bool,
    gate: &kiss::GateConfig,
) -> Result<crate::test_runner::check_line_coverage::ValidatedCovInputs, i32> {
    use crate::test_runner::check_line_coverage::ValidatedCovInputs;
    let snapshot = match load_check_runtime_coverage(repo_root, required, ignore, gate) {
        Ok(snapshot) => snapshot,
        Err(load_err) => {
            if !allow_refresh {
                eprintln!("{load_err}");
                return Err(1);
            }
            ensure_check_runtime_coverage(repo_root, required, ignore, jobs).map_err(|err| {
                eprintln!("{err}");
                1
            })?;
            load_check_runtime_coverage(repo_root, required, ignore, gate).map_err(|err| {
                eprintln!("{err}");
                1
            })?
        }
    };
    Ok(ValidatedCovInputs::from_snapshot(required, snapshot, repo_root))
}

struct SiblingGateResult {
    coverage_failed: bool,
    time_failed: bool,
}

fn finish_sibling_gates(result: SiblingGateResult) -> i32 {
    print_final_status(result.coverage_failed || result.time_failed);
    i32::from(result.coverage_failed || result.time_failed)
}

fn evaluate_time_gate_for_cov(
    args: &CovCommandArgs<'_>,
    universe_root: &Path,
    files: &CovFileSets,
    ignore: &[String],
) -> RuntimeGateEval {
    if args.gate_config.unit_test_time_gate_disabled() {
        return RuntimeGateEval::Disabled;
    }
    evaluate_cov_time_gate(CovTimeGateOpts {
        universe: universe_root,
        lang_filter: args.lang_filter,
        include: TimingLangInclude {
            python: !files.py_files.is_empty(),
            rust: !files.rs_files.is_empty(),
        },
        ignore,
        limits: &args.gate_config.max_unit_test_seconds,
        timing: args.timing,
    })
}

fn apply_time_gate_eval(eval: &RuntimeGateEval) -> bool {
    match eval {
        RuntimeGateEval::Disabled | RuntimeGateEval::Passed => false,
        RuntimeGateEval::Failed(viols) => {
            for line in runtime_gate_failure_lines(viols) {
                println!("{line}");
            }
            true
        }
        RuntimeGateEval::Incomplete => {
            eprintln!(
                "error: kiss test: unit-test timing cache is incomplete for the current population"
            );
            true
        }
    }
}

fn evaluate_coverage_gate(
    records: &[analyze::line_coverage::LineCoverageRecord],
    focus: &analyze::FocusFilter,
    threshold: usize,
    scope: kiss::TestCoverageScope,
    bypass_gate: bool,
) -> bool {
    if !bypass_gate
        && threshold > 0
        && let Some(result) = analyze::evaluate_line_gate(records, focus, threshold, scope)
    {
        return !result.success;
    }
    let viols = analyze::collect_line_coverage_viols(records, focus, bypass_gate);
    print_violations(&viols);
    !viols.is_empty()
}

struct RecordsEvalCtx<'a> {
    focus: &'a analyze::FocusFilter,
    threshold: usize,
    scope: kiss::TestCoverageScope,
    args: &'a CovCommandArgs<'a>,
    universe_root: &'a Path,
    files: &'a CovFileSets,
    ignore: &'a [String],
}

fn evaluate_records_with_time(
    records: &[analyze::line_coverage::LineCoverageRecord],
    ctx: &RecordsEvalCtx<'_>,
) -> i32 {
    let coverage_failed = evaluate_coverage_gate(
        records,
        ctx.focus,
        ctx.threshold,
        ctx.scope,
        ctx.args.bypass_gate,
    );
    let time_eval =
        evaluate_time_gate_for_cov(ctx.args, ctx.universe_root, ctx.files, ctx.ignore);
    let time_failed =
        apply_time_gate_eval(&time_eval);
    finish_sibling_gates(SiblingGateResult {
        coverage_failed,
        time_failed,
    })
}

/// Like [`evaluate_records_with_time`], but returns `None` when the time gate needs a
/// cold population refresh (incomplete durable timings) so the caller can fall through.
fn try_evaluate_records_with_time(
    records: &[analyze::line_coverage::LineCoverageRecord],
    ctx: &RecordsEvalCtx<'_>,
) -> Option<i32> {
    let time_eval =
        evaluate_time_gate_for_cov(ctx.args, ctx.universe_root, ctx.files, ctx.ignore);
    if matches!(time_eval, RuntimeGateEval::Incomplete) {
        if !ctx.args.allow_refresh {
            eprintln!(
                "error: kiss test: unit-test timing cache is incomplete for the current population"
            );
            return Some(1);
        }
        // Schema bump / stale entries: force cold path so coverage can refresh once.
        return None;
    }
    let coverage_failed = evaluate_coverage_gate(
        records,
        ctx.focus,
        ctx.threshold,
        ctx.scope,
        ctx.args.bypass_gate,
    );
    let time_failed =
        apply_time_gate_eval(&time_eval);
    Some(finish_sibling_gates(SiblingGateResult {
        coverage_failed,
        time_failed,
    }))
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
    let repo_root = repository_root_for_universe(universe_root);
    let list_key = crate::analyze::cov_file_list_cache::CovFileListKey {
        repo_root: &repo_root,
        lang_filter,
        ignore,
    };
    let (py_files, mut rs_files) =
        if let Some(cached) = crate::analyze::cov_file_list_cache::try_load_cov_file_list(&list_key)
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
    rs_files = super::cov_workspace_files::filter_root_workspace_rust_cov_files(&repo_root, rs_files);
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
    crate::test_runner::python_coverage_index::clear_python_generation_warm_memo();
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
    if threshold == 0
        && args.gate_config.unit_test_time_gate_disabled()
        && !args.bypass_gate
    {
        print_final_status(false);
        return 0;
    }
    // Time-only: skip line-coverage record computation when coverage gate and --all
    // do not need records.
    if threshold == 0 && !args.bypass_gate {
        let repo_root = repository_root_for_universe(universe_root);
        let required = RequiredCoverageLanguages {
            python: !files.py_files.is_empty(),
            rust: !files.rs_files.is_empty(),
        };
        // Ensure populations are current so schema bumps can refresh durations once.
        let _ = load_or_refresh_snapshot(
            &repo_root,
            required,
            &ignore,
            args.jobs,
            args.allow_refresh,
            args.gate_config,
        );
        let time_eval = evaluate_time_gate_for_cov(args, universe_root, &files, &ignore);
        let time_failed = apply_time_gate_eval(&time_eval);
        return finish_sibling_gates(SiblingGateResult {
            coverage_failed: false,
            time_failed,
        });
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
    let eval_ctx = RecordsEvalCtx {
        focus: &focus,
        threshold: p.threshold,
        scope,
        args: p.args,
        universe_root: p.universe_root,
        files: p.files,
        ignore: p.ignore,
    };
    if let Some(code) = try_cov_records_fast_path(&cache_key, &eval_ctx, t0) {
        return code;
    }
    let validated = match load_or_refresh_snapshot(
        &repo_root,
        required,
        p.ignore,
        p.args.jobs,
        p.args.allow_refresh,
        p.args.gate_config,
    ) {
        Ok(validated) => validated,
        Err(code) => return code,
    };
    if p.args.timing
        && let Some(id) = validated.python_generation_id.as_ref()
    {
        eprintln!("TIMING:python_generation_id:{id}");
    }
    let records = compute_and_store_records(
        &cache_key,
        &repo_root,
        p.files,
        &validated.snapshot,
        p.args.timing,
        t0,
    );
    evaluate_records_with_time(&records, &eval_ctx)
}

/// Hit `cov_records_cache` when present under `./.kiss`.
fn try_cov_records_fast_path(
    cache_key: &CovRecordsCacheKey<'_>,
    ctx: &RecordsEvalCtx<'_>,
    t0: Instant,
) -> Option<i32> {
    let records = try_load_cov_records(cache_key)?;
    if ctx.args.timing {
        eprintln!(
            "TIMING:coverage_records_cache_hit_ms:{}",
            t0.elapsed().as_millis()
        );
    }
    try_evaluate_records_with_time(&records, ctx)
}

#[cfg(test)]
#[path = "cov_cmd_test.rs"]
mod tests;
