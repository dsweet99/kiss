use crate::analyze;
use crate::analyze::build_focus_filter;
use crate::analyze::cov_records_cache::{
    CovRecordsCacheKey, mark_cached_records_orphan_clean, try_load_cov_records_with_orphan_state,
};
use crate::bin_cli::cov_cmd_cache::{
    compute_and_store_records, gather_cov_files, lang_filter_cache_label,
};
use crate::bin_cli::cov_sibling_gates::{
    SiblingGateResult, apply_time_gate_eval, evaluate_max_num_tests_gate,
    evaluate_time_gate_for_cov, finish_sibling_gates,
};
use crate::bin_cli::util::{merge_check_ignore_prefixes, validate_paths};
use crate::test_runner::check_line_coverage::{
    RequiredCoverageLanguages, ensure_check_runtime_coverage, load_check_runtime_coverage,
    repository_root_for_universe,
};
use crate::test_runner::unit_test_timing::RuntimeGateEval;
use kiss::Language;
use kiss::cli_output::{print_no_files_message, print_violations};
use std::path::Path;
use std::time::Instant;

pub(crate) use crate::bin_cli::cov_cmd_cache::CovFileSets;

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
    pub allow_refresh: bool,
    pub pytest_args: &'a [String],
    pub language_tables: kiss::LanguageTablesPresent,
}

fn load_or_refresh_snapshot(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
    jobs: usize,
    allow_refresh: bool,
    gate: &kiss::GateConfig,
    pytest_args: &[String],
) -> Result<crate::test_runner::check_line_coverage::ValidatedCovInputs, i32> {
    use crate::test_runner::check_line_coverage::ValidatedCovInputs;
    let snapshot =
        match load_check_runtime_coverage(repo_root, required, ignore, gate, pytest_args) {
            Ok(snapshot) => snapshot,
            Err(load_err) => {
                if !allow_refresh {
                    eprintln!("{load_err}");
                    return Err(1);
                }
                ensure_check_runtime_coverage(repo_root, required, ignore, jobs, pytest_args, gate)
                    .map_err(|err| {
                        eprintln!("{err}");
                        1
                    })?;
                load_check_runtime_coverage(repo_root, required, ignore, gate, pytest_args)
                    .map_err(|err| {
                        eprintln!("{err}");
                        1
                    })?
            }
        };
    Ok(ValidatedCovInputs::from_snapshot(
        required, snapshot, repo_root,
    ))
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

fn orphan_gate_failed(
    ctx: &RecordsEvalCtx<'_>,
    snapshot: Option<&analyze::line_coverage::RuntimeCoverageSnapshot>,
) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };
    let repo_root = repository_root_for_universe(ctx.universe_root);
    analyze::evaluate_orphan_unit_gate(
        &repo_root,
        &ctx.files.py_files,
        &ctx.files.rs_files,
        snapshot,
        ctx.args.gate_config,
        ctx.args.bypass_gate,
    )
}

fn evaluate_records_with_time(
    records: &[analyze::line_coverage::LineCoverageRecord],
    ctx: &RecordsEvalCtx<'_>,
    snapshot: Option<&analyze::line_coverage::RuntimeCoverageSnapshot>,
) -> i32 {
    let orphan_failed = orphan_gate_failed(ctx, snapshot);
    let coverage_failed = evaluate_coverage_gate(
        records,
        ctx.focus,
        ctx.threshold,
        ctx.scope,
        ctx.args.bypass_gate,
    );
    let time_eval = evaluate_time_gate_for_cov(ctx.args, ctx.universe_root, ctx.files, ctx.ignore);
    let time_failed = apply_time_gate_eval(&time_eval);
    let max_num_tests_failed =
        evaluate_max_num_tests_gate(ctx.args, ctx.universe_root, ctx.files, ctx.ignore);
    finish_sibling_gates(SiblingGateResult {
        coverage_failed,
        time_failed,
        max_num_tests_failed,
        orphan_failed,
    })
}

#[cfg(test)]
fn try_evaluate_records_with_time(
    records: &[analyze::line_coverage::LineCoverageRecord],
    ctx: &RecordsEvalCtx<'_>,
) -> Option<i32> {
    try_evaluate_records_with_orphan_state(records, ctx, false)
}

fn try_evaluate_records_with_orphan_state(
    records: &[analyze::line_coverage::LineCoverageRecord],
    ctx: &RecordsEvalCtx<'_>,
    orphan_clean: bool,
) -> Option<i32> {
    if ctx.args.gate_config.orphan_detection && !ctx.args.bypass_gate && !orphan_clean {
        return None;
    }
    let time_eval = evaluate_time_gate_for_cov(ctx.args, ctx.universe_root, ctx.files, ctx.ignore);
    if matches!(time_eval, RuntimeGateEval::Incomplete) {
        if !ctx.args.allow_refresh {
            eprintln!(
                "error: kiss test: unit-test timing cache is incomplete for the current population"
            );
            return Some(1);
        }

        return None;
    }
    let coverage_failed = evaluate_coverage_gate(
        records,
        ctx.focus,
        ctx.threshold,
        ctx.scope,
        ctx.args.bypass_gate,
    );
    let time_failed = apply_time_gate_eval(&time_eval);
    let max_num_tests_failed =
        evaluate_max_num_tests_gate(ctx.args, ctx.universe_root, ctx.files, ctx.ignore);
    Some(finish_sibling_gates(SiblingGateResult {
        coverage_failed,
        time_failed,
        max_num_tests_failed,
        orphan_failed: false,
    }))
}

#[allow(dead_code)]
pub fn run_cov_command(args: &CovCommandArgs<'_>) -> i32 {
    run_cov_command_impl(args, true)
}

pub(crate) fn run_cov_command_impl(args: &CovCommandArgs<'_>, print_empty: bool) -> i32 {
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
        if print_empty {
            print_no_files_message(args.lang_filter, universe_root);
        }
        return 0;
    };
    if let Err(code) = crate::bin_cli::util::reject_unconfigured_languages(
        &files.py_files,
        &files.rs_files,
        args.language_tables,
    ) {
        return code;
    }
    if args.timing {
        eprintln!(
            "TIMING:coverage_gather_files_ms:{}",
            t_gather.elapsed().as_millis()
        );
    }
    let threshold = args.gate_config.test_coverage_threshold;

    if threshold == 0 && !args.bypass_gate {
        let repo_root = repository_root_for_universe(universe_root);
        let required = RequiredCoverageLanguages {
            python: !files.py_files.is_empty(),
            rust: !files.rs_files.is_empty(),
        };

        let validated = load_or_refresh_snapshot(
            &repo_root,
            required,
            &ignore,
            args.jobs,
            args.allow_refresh,
            args.gate_config,
            args.pytest_args,
        );
        let orphan_failed = match validated {
            Ok(inputs) => analyze::evaluate_orphan_unit_gate(
                &repo_root,
                &files.py_files,
                &files.rs_files,
                &inputs.snapshot,
                args.gate_config,
                args.bypass_gate,
            ),
            Err(_) => args.gate_config.orphan_detection && !args.bypass_gate,
        };
        let time_eval = evaluate_time_gate_for_cov(args, universe_root, &files, &ignore);
        let time_failed = apply_time_gate_eval(&time_eval);
        let max_num_tests_failed =
            evaluate_max_num_tests_gate(args, universe_root, &files, &ignore);
        return finish_sibling_gates(SiblingGateResult {
            coverage_failed: false,
            time_failed,
            max_num_tests_failed,
            orphan_failed,
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
        pytest_args: p.args.pytest_args,
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
    if let Some((records, cached_orphan_policy)) =
        try_load_cov_records_with_orphan_state(&cache_key)
    {
        let orphan_clean =
            cached_orphan_policy == orphan_policy(&p.args.gate_config.orphan_allowed);
        if p.args.timing {
            eprintln!(
                "TIMING:coverage_records_cache_hit_ms:{}",
                t0.elapsed().as_millis()
            );
        }
        if let Some(code) =
            try_evaluate_records_with_orphan_state(&records, &eval_ctx, orphan_clean)
        {
            return code;
        }
        if let Some(code) = evaluate_cached_records_for_orphan(
            &records, &eval_ctx, &repo_root, required, &cache_key, &t0,
        ) {
            return code;
        }
    }
    let validated = match load_or_refresh_snapshot(
        &repo_root,
        required,
        p.ignore,
        p.args.jobs,
        p.args.allow_refresh,
        p.args.gate_config,
        p.args.pytest_args,
    ) {
        Ok(validated) => validated,
        Err(code) => return code,
    };
    if p.args.timing
        && let Some(id) = validated.python_generation_id.as_ref()
    {
        eprintln!("TIMING:python_generation_id:{id}");
    }
    let records = match compute_and_store_records(
        &cache_key,
        &repo_root,
        p.files,
        &validated.snapshot,
        p.args.timing,
        t0,
    ) {
        Ok(records) => records,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };
    evaluate_records_with_time(&records, &eval_ctx, Some(&validated.snapshot))
}

fn evaluate_cached_records_for_orphan(
    records: &[analyze::line_coverage::LineCoverageRecord],
    eval_ctx: &RecordsEvalCtx<'_>,
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    cache_key: &CovRecordsCacheKey<'_>,
    t0: &Instant,
) -> Option<i32> {
    if !eval_ctx.args.gate_config.orphan_detection || eval_ctx.args.bypass_gate {
        return None;
    }
    let time_eval = evaluate_time_gate_for_cov(
        eval_ctx.args,
        eval_ctx.universe_root,
        eval_ctx.files,
        eval_ctx.ignore,
    );
    if matches!(time_eval, RuntimeGateEval::Incomplete) {
        return None;
    }
    let validated = match load_or_refresh_snapshot(
        repo_root,
        required,
        eval_ctx.ignore,
        eval_ctx.args.jobs,
        eval_ctx.args.allow_refresh,
        eval_ctx.args.gate_config,
        eval_ctx.args.pytest_args,
    ) {
        Ok(validated) => validated,
        Err(code) => return Some(code),
    };
    if eval_ctx.args.timing
        && let Some(id) = validated.python_generation_id.as_ref()
    {
        eprintln!("TIMING:python_generation_id:{id}");
    }
    if eval_ctx.args.timing {
        eprintln!(
            "TIMING:coverage_snapshot_load_or_refresh_ms:{}",
            t0.elapsed().as_millis()
        );
    }
    let orphan_failed = orphan_gate_failed(eval_ctx, Some(&validated.snapshot));
    if !orphan_failed {
        mark_cached_records_orphan_clean(
            cache_key,
            &orphan_policy(&eval_ctx.args.gate_config.orphan_allowed),
        );
    }
    Some(evaluate_records_with_time(
        records,
        eval_ctx,
        Some(&validated.snapshot),
    ))
}

fn orphan_policy(orphan_allowed: &[String]) -> String {
    format!("orphan-policy-v1:{}", orphan_allowed.join("\0"))
}
#[cfg(test)]
#[path = "cov_cmd_refresh_test.rs"]
mod refresh_tests;
#[cfg(test)]
#[path = "cov_cmd_test.rs"]
mod tests;
