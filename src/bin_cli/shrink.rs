use crate::analyze;
use crate::analyze::GlobalMetricsInput;
use crate::bin_cli::util::{normalize_ignore_prefixes, validate_paths};
use kiss::{
    check_shrink_constraints, parse_target_arg, ShrinkState, ShrinkViolations,
};

pub use super::shrink_analysis_types::{ShrinkAnalyzeArgs, ShrinkMetricsArgs};
pub use super::shrink_types::{RunShrinkArgs, ShrinkFullContext, ShrinkStartContext};

pub fn run_shrink(args: RunShrinkArgs<'_>) -> i32 {
    match &args.target {
        None => run_shrink_check(args.paths, args.ignore, args.ctx),
        Some(t) => {
            let start = ShrinkStartContext {
                lang_filter: args.ctx.lang_filter,
                py_config: args.ctx.py_config,
                rs_config: args.ctx.rs_config,
            };
            run_shrink_start(t, args.paths, args.ignore, &start)
        }
    }
}

pub fn run_shrink_start(
    target_arg: &str,
    paths: &[String],
    ignore: &[String],
    ctx: &ShrinkStartContext<'_>,
) -> i32 {
    let ignore = normalize_ignore_prefixes(ignore);
    validate_paths(paths);

    let (target, target_value) = match parse_target_arg(target_arg) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let in_ = GlobalMetricsInput {
        paths,
        ignore: &ignore,
        lang_filter: ctx.lang_filter,
        py_config: ctx.py_config,
        rs_config: ctx.rs_config,
    };
    let Some(current) = analyze::compute_global_metrics(&in_) else {
        eprintln!("Error: No source files found.");
        return 1;
    };

    let current_target_value = target.get(&current);
    if target_value > current_target_value {
        eprintln!(
            "Error: Target value {} exceeds current value {} for {}. Can only shrink, not grow.",
            target_value,
            current_target_value,
            target.as_str()
        );
        return 1;
    }

    let state = ShrinkState {
        baseline: current,
        target,
        target_value,
    };
    if let Err(e) = state.save() {
        eprintln!("Error saving .kiss_shrink: {e}");
        return 1;
    }

    println!(
        "SHRINK_STARTED: target={} value={} current={}",
        target.as_str(),
        target_value,
        current_target_value
    );
    println!(
        "SHRINK_CONSTRAINTS: files<={} code_units<={} statements<={} graph_nodes<={} graph_edges<={}",
        current.files,
        current.code_units,
        current.statements,
        current.graph_nodes,
        current.graph_edges
    );
    println!("SHRINK_SAVED: .kiss_shrink");
    0
}

pub fn run_shrink_check(
    paths: &[String],
    ignore: &[String],
    ctx: &ShrinkFullContext<'_>,
) -> i32 {
    let ignore = normalize_ignore_prefixes(ignore);
    validate_paths(paths);

    let Some(state) = ShrinkState::load() else {
        eprintln!("Error: No .kiss_shrink file found. Run 'kiss shrink <METRIC>=<VALUE>' first.");
        return 1;
    };

    let result = run_shrink_analysis(ShrinkAnalyzeArgs {
        paths,
        ignore: &ignore,
        ctx,
    });
    let current = get_shrink_metrics(ShrinkMetricsArgs {
        result: &result,
        paths,
        ignore: &ignore,
        ctx,
    });
    let Some(current) = current else { return 1 };

    let shrink_result = check_shrink_constraints(&state, &current);
    for v in &shrink_result.violations {
        println!("{v}");
    }
    print_shrink_progress(&state, &current);

    emit_shrink_final_status(result.success, &shrink_result)
}

pub fn run_shrink_analysis(args: ShrinkAnalyzeArgs<'_, '_>) -> analyze::AnalyzeResult {
    let universe = &args.paths[0];
    let focus = if args.paths.len() > 1 {
        &args.paths[1..]
    } else {
        args.paths
    };
    let opts = analyze::AnalyzeOptions {
        universe,
        focus_paths: focus,
        py_config: args.ctx.py_config,
        rs_config: args.ctx.rs_config,
        lang_filter: args.ctx.lang_filter,
        bypass_gate: false,
        gate_config: args.ctx.gate_config,
        ignore_prefixes: args.ignore,
        show_timing: false,
        suppress_final_status: true,
    };
    analyze::run_analyze_with_result(&opts)
}

pub fn get_shrink_metrics(args: ShrinkMetricsArgs<'_, '_>) -> Option<kiss::GlobalMetrics> {
    if let Some(m) = args.result.metrics {
        return Some(m);
    }
    let in_ = GlobalMetricsInput {
        paths: args.paths,
        ignore: args.ignore,
        lang_filter: args.ctx.lang_filter,
        py_config: args.ctx.py_config,
        rs_config: args.ctx.rs_config,
    };
    let m = analyze::compute_global_metrics(&in_)?;
    if m.files == 0 {
        eprintln!("Error: No source files found.");
        None
    } else {
        Some(m)
    }
}

pub fn print_shrink_progress(state: &ShrinkState, current: &kiss::GlobalMetrics) {
    let baseline_target = state.target.get(&state.baseline);
    let current_target = state.target.get(current);
    println!(
        "SHRINK_PROGRESS: metric={} baseline={} current={} target={}",
        state.target.as_str(),
        baseline_target,
        current_target,
        state.target_value
    );
}

pub fn emit_shrink_final_status(check_ok: bool, shrink_result: &ShrinkViolations) -> i32 {
    let has_failures = !check_ok || !shrink_result.violations.is_empty();
    if !has_failures {
        println!("NO VIOLATIONS");
    }
    i32::from(has_failures)
}
