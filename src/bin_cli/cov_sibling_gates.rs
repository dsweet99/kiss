//! Sibling gates evaluated with coverage under `kiss test` / `kiss cov`.

use std::path::Path;

use kiss::Language;
use kiss::cli_output::print_final_status;

use crate::bin_cli::cov_cmd::{CovCommandArgs, CovFileSets};
use crate::test_runner::unit_test_timing::{
    CovTimeGateOpts, RuntimeGateEval, TimingLangInclude, evaluate_cov_time_gate,
    runtime_gate_failure_lines,
};

pub(crate) struct SiblingGateResult {
    pub(crate) coverage_failed: bool,
    pub(crate) time_failed: bool,
    pub(crate) max_num_tests_failed: bool,
}

pub(crate) fn finish_sibling_gates(result: SiblingGateResult) -> i32 {
    print_final_status(
        result.coverage_failed || result.time_failed || result.max_num_tests_failed,
    );
    i32::from(result.coverage_failed || result.time_failed || result.max_num_tests_failed)
}

pub(crate) fn evaluate_time_gate_for_cov(
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
        pytest_args: args.pytest_args,
    })
}

pub(crate) fn apply_time_gate_eval(eval: &RuntimeGateEval) -> bool {
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

pub(crate) fn evaluate_max_num_tests_gate(
    args: &CovCommandArgs<'_>,
    universe_root: &Path,
    ignore: &[String],
) -> bool {
    if args.bypass_gate {
        return false;
    }
    let Some(count) = crate::test_runner::unit_test_timing::codebase_test_count_for_cov(
        universe_root,
        args.lang_filter,
        TimingLangInclude {
            python: matches!(args.lang_filter, None | Some(Language::Python)),
            rust: matches!(args.lang_filter, None | Some(Language::Rust)),
        },
        ignore,
        args.pytest_args,
    ) else {
        eprintln!(
            "error: kiss test: unit-test population count is unavailable for max_num_tests"
        );
        return true;
    };
    let limit = args.gate_config.max_num_tests;
    if count > limit {
        println!(
            "VIOLATION:max_num_tests: {count} test(s) exceeds max_num_tests={limit}"
        );
        true
    } else {
        false
    }
}

#[cfg(test)]
#[path = "cov_sibling_gates_test.rs"]
mod tests;
