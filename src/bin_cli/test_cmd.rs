use std::cell::Cell;
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

thread_local! {
    /// Test-only: when set, `run_test_command` uses this instead of probing/nudging W.
    static CLIENT_RESULT_OVERRIDE: Cell<Option<Result<Option<i32>, String>>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_client_result_override_for_test(value: Option<Result<Option<i32>, String>>) {
    CLIENT_RESULT_OVERRIDE.with(|c| c.set(value));
}

pub fn run_test_command(args: TestCommandArgs<'_>) -> i32 {
    run_test_command_with(args, run_test)
}

/// Injectable local runner for unit tests (assert client path never calls it).
pub(crate) fn run_test_command_with(
    args: TestCommandArgs<'_>,
    run_local: impl FnOnce(RunTestCmdArgs<'_>) -> i32,
) -> i32 {
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
        gate_config: args.gate_config.clone(),
    };
    if args.watch {
        let settle = Duration::from_secs_f64(args.test_cfg.watch_settle_seconds);
        return run_test_watch(run_args, settle);
    }
    if args.dry_run {
        return run_local(run_args);
    }

    #[cfg(unix)]
    {
        match try_run_as_watcher_client(&args) {
            Ok(Some(code)) => return code,
            Ok(None) => {}
            Err(e) => {
                eprintln!("error: kiss test: {e}");
                return 1;
            }
        }
    }

    let code = run_local(run_args);
    if code != 0 {
        return code;
    }
    finish_with_coverage(&args, code)
}

/// If a live watcher owns the repo, nudge it and evaluate coverage from cache.
///
/// Returns `Ok(None)` when no watcher is present (caller runs tests locally).
#[cfg(unix)]
fn try_run_as_watcher_client(args: &TestCommandArgs<'_>) -> Result<Option<i32>, String> {
    if let Some(overridden) = CLIENT_RESULT_OVERRIDE.with(Cell::take) {
        return match overridden {
            Ok(None) => Ok(None),
            Ok(Some(exit_code)) => Ok(Some(apply_watcher_client_exit(args, exit_code))),
            Err(e) => Err(e),
        };
    }

    use crate::test_runner::{NudgeRequestMsg, nudge_watcher_with_retry, probe_live_watcher};

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd)?;
    let Some(session) = probe_live_watcher(&repo_root)? else {
        return Ok(None);
    };
    println!("kiss test: waiting for watcher (pid {})", session.pid);
    let reply = nudge_watcher_with_retry(
        &repo_root,
        &session,
        &NudgeRequestMsg {
            force: args.force,
            force_bad: args.force_bad,
            metrics: args.metrics,
        },
    )?;
    Ok(Some(apply_watcher_client_exit(args, reply.exit_code)))
}

#[cfg(unix)]
fn apply_watcher_client_exit(args: &TestCommandArgs<'_>, exit_code: i32) -> i32 {
    println!("kiss test: watcher cycle complete");
    if exit_code != 0 {
        println!("FAIL");
        return exit_code;
    }
    println!("PASS");
    finish_with_coverage(args, exit_code)
}

/// Client path after a successful watcher nudge: evaluate coverage from cache only.
pub(crate) fn finish_with_coverage(args: &TestCommandArgs<'_>, test_exit: i32) -> i32 {
    let universe = universe_root_for_test_invocation(&args.invocation);
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
    if cov_code != 0 {
        cov_code
    } else {
        test_exit
    }
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
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    #[cfg(unix)]
    #[test]
    fn injected_client_result_skips_local_run_test() {
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::All,
            main_branch: None,
            base_branch: None,
            dry_run: false,
            force: false,
            force_bad: false,
            metrics: false,
            coverage_all: false,
            watch: false,
            jobs: 1,
            ignore: &[],
            extra: &[],
            lang_filter: Some(kiss::Language::Python),
            test_cfg: &test_cfg,
            py_config: &py,
            rs_config: &rs,
            gate_config: &gate,
        };
        set_client_result_override_for_test(Some(Ok(Some(9))));
        let calls = AtomicUsize::new(0);
        let code = run_test_command_with(args, |_a| {
            calls.fetch_add(1, Ordering::SeqCst);
            0
        });
        set_client_result_override_for_test(None);
        assert_eq!(code, 9);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "client path must not call the local test runner"
        );
    }
}
