use std::path::PathBuf;
use std::time::Duration;

use kiss::TestSectionConfig;

use crate::bin_cli::args::TestInvocation;
use crate::bin_cli::cov_cmd::{CovCommandArgs, run_cov_command};
use crate::bin_cli::cov_warm::warm_cov_caches_after_tests;
use crate::test_runner::{RunTestCmdArgs, run_test, run_test_watch};

pub struct TestCommandArgs<'a> {
    pub invocation: TestInvocation,
    pub main_branch: Option<&'a str>,
    pub base_branch: Option<&'a str>,
    pub dry_run: bool,
    pub force: bool,
    pub force_bad: bool,
    pub metrics: bool,
    pub coverage_all: bool,
    pub watch: bool,
    pub jobs: usize,
    pub ignore: &'a [String],
    pub extra: &'a [String],
    pub lang_filter: Option<kiss::Language>,
    pub test_cfg: &'a TestSectionConfig,
    pub py_config: &'a kiss::Config,
    pub rs_config: &'a kiss::Config,
    pub gate_config: &'a kiss::GateConfig,
}

pub fn run_test_command(args: TestCommandArgs<'_>) -> i32 {
    let python_extra_owned =
        kiss::effective_python_pytest_args(&args.test_cfg.pytest_plugins, args.extra);
    let run_args = RunTestCmdArgs {
        invocation: args.invocation.clone(),
        main_branch_cli: args.main_branch,
        base_branch_cli: args.base_branch,
        dry_run: args.dry_run,
        force_rerun: args.force,
        force_bad: args.force_bad,
        metrics: args.metrics,
        jobs: args.jobs,
        extra: args.extra,
        python_extra: &python_extra_owned,
        ignore: args.ignore,
        lang_filter: args.lang_filter,
        config_main_branch: args.test_cfg.main_branch.as_deref(),
    };
    if args.watch {
        let settle = Duration::from_secs_f64(args.test_cfg.watch_settle_seconds);
        return run_test_watch(run_args, settle);
    }
    let code = run_test(run_args);
    if code != 0 || args.dry_run {
        return code;
    }
    // After a successful run the population is current; evaluate coverage and
    // unit-test time gates (formerly `kiss cov`) against that cache.
    let universe = universe_root_for_test_invocation(&args.invocation);
    // Prime file-list/records caches from the just-ensured population so coverage
    // evaluation can stay on the warm read path without a second ensure.
    warm_cov_caches_after_tests(&universe, args.lang_filter, args.ignore);
    let universe_s = universe.to_string_lossy().into_owned();
    let paths = [universe_s];
    let cov_code = run_cov_command(&CovCommandArgs {
        paths: &paths,
        lang_filter: args.lang_filter,
        py_config: args.py_config,
        rs_config: args.rs_config,
        gate_config: args.gate_config,
        bypass_gate: args.coverage_all,
        ignore: args.ignore,
        timing: false,
        jobs: args.jobs,
        allow_refresh: false,
    });
    if cov_code != 0 { cov_code } else { code }
}

/// Universe root for post-test cov warming: single path/dir target when present,
/// otherwise the repository root (`.`).
fn universe_root_for_test_invocation(invocation: &TestInvocation) -> PathBuf {
    match invocation {
        TestInvocation::Targets(targets) if targets.len() == 1 => {
            let raw = &targets[0];
            let path_part = raw.split_once("::").map_or(raw.as_str(), |(p, _)| p);
            if path_part.is_empty() || path_part == "." || path_part == "./" {
                PathBuf::from(".")
            } else {
                PathBuf::from(path_part)
            }
        }
        _ => PathBuf::from("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn universe_root_defaults_to_dot_for_all_modes() {
        assert_eq!(
            universe_root_for_test_invocation(&TestInvocation::All),
            PathBuf::from(".")
        );
        assert_eq!(
            universe_root_for_test_invocation(&TestInvocation::Commit),
            PathBuf::from(".")
        );
    }

    #[test]
    fn universe_root_uses_single_path_target() {
        assert_eq!(
            universe_root_for_test_invocation(&TestInvocation::Targets(vec!["src/foo".into()])),
            PathBuf::from("src/foo")
        );
        assert_eq!(
            universe_root_for_test_invocation(&TestInvocation::Targets(vec![
                "src/lib.rs::my_test".into()
            ])),
            PathBuf::from("src/lib.rs")
        );
    }
}
