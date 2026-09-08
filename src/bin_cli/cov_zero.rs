use std::path::Path;

use crate::analyze;
use crate::bin_cli::cov_sibling_gates::{
    SiblingGateResult, apply_time_gate_eval, evaluate_max_num_tests_gate,
    evaluate_time_gate_for_cov, finish_sibling_gates,
};
use crate::test_runner::check_line_coverage::{
    RequiredCoverageLanguages, repository_root_for_universe,
};
use crate::test_runner::unit_test_timing::{TimingLangInclude, codebase_test_count_for_cov};

use super::{CovCommandArgs, CovFileSets, load_or_refresh_snapshot};

pub(super) fn finish_zero_threshold_cov(
    args: &CovCommandArgs<'_>,
    universe_root: &Path,
    files: &CovFileSets,
    ignore: &[String],
) -> i32 {
    finish_sibling_gates(SiblingGateResult {
        coverage_failed: false,
        time_failed: apply_time_gate_eval(&evaluate_time_gate_for_cov(
            args,
            universe_root,
            files,
            ignore,
        )),
        max_num_tests_failed: max_num_tests_failed_zero_threshold(
            args,
            universe_root,
            files,
            ignore,
        ),
        orphan_failed: orphan_failed_zero_threshold(args, universe_root, files, ignore),
    })
}

fn include_from_files(files: &CovFileSets) -> TimingLangInclude {
    TimingLangInclude {
        python: !files.py_files.is_empty(),
        rust: !files.rs_files.is_empty(),
    }
}

fn orphan_failed_zero_threshold(
    args: &CovCommandArgs<'_>,
    universe_root: &Path,
    files: &CovFileSets,
    ignore: &[String],
) -> bool {
    if !args.gate_config.orphan_detection {
        return false;
    }
    let repo_root = repository_root_for_universe(universe_root);
    let required = RequiredCoverageLanguages {
        python: !files.py_files.is_empty(),
        rust: !files.rs_files.is_empty(),
    };
    match load_or_refresh_snapshot(
        &repo_root,
        required,
        ignore,
        args.jobs,
        args.allow_refresh,
        args.gate_config,
        args.pytest_args,
    ) {
        Ok(inputs) => analyze::evaluate_orphan_unit_gate(
            &repo_root,
            &files.py_files,
            &files.rs_files,
            &inputs.snapshot,
            args.gate_config,
            args.bypass_gate,
        ),
        Err(_) => true,
    }
}

fn max_num_tests_failed_zero_threshold(
    args: &CovCommandArgs<'_>,
    universe_root: &Path,
    files: &CovFileSets,
    ignore: &[String],
) -> bool {
    let include = include_from_files(files);
    if codebase_test_count_for_cov(
        universe_root,
        args.lang_filter,
        include,
        ignore,
        args.pytest_args,
    )
    .is_none()
        && args.allow_refresh
    {
        let repo_root = repository_root_for_universe(universe_root);
        let required = RequiredCoverageLanguages {
            python: include.python,
            rust: include.rust,
        };
        let _ = load_or_refresh_snapshot(
            &repo_root,
            required,
            ignore,
            args.jobs,
            args.allow_refresh,
            args.gate_config,
            args.pytest_args,
        );
    }
    evaluate_max_num_tests_gate(args, universe_root, files, ignore)
}
