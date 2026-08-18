use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;

use kiss::TestSectionConfig;

use crate::bin_cli::args::TestInvocation;
use crate::bin_cli::cov_cmd::{CovCommandArgs, run_cov_command};
use crate::bin_cli::cov_warm::warm_cov_caches_after_tests;
use crate::test_runner::{
    RunTestCmdArgs, WatchCoverageParams, WatchCoverageResult, run_test, run_test_watch,
};

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
    /// Explicit `--jobs` when set; `None` means jobs came from config (reloadable).
    pub jobs_cli: Option<usize>,
    pub ignore: &'a [String],
    /// CLI `--ignore` only (before config merge); used when reloading `.kissconfig`.
    pub cli_ignore: &'a [String],
    pub extra: &'a [String],
    pub lang_filter: Option<kiss::Language>,
    pub test_cfg: &'a TestSectionConfig,
    pub py_config: &'a kiss::Config,
    pub rs_config: &'a kiss::Config,
    pub gate_config: &'a kiss::GateConfig,
    /// When false (`--defaults`), the watcher does not reload `.kissconfig`.
    pub reload_kissconfig: bool,
    pub config_path: Option<&'a PathBuf>,
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
        let seed = crate::test_runner::WatchReloadSeed {
            cli_ignore: args.cli_ignore.to_vec(),
            jobs_cli: args.jobs_cli,
            extra: args.extra.to_vec(),
            coverage_all: args.coverage_all,
            enabled: args.reload_kissconfig,
            config_path: args
                .config_path
                .cloned()
                .unwrap_or_else(|| PathBuf::from(".kissconfig")),
        };
        return run_test_watch(
            run_args,
            settle,
            seed,
            args.py_config.clone(),
            args.rs_config.clone(),
            |cycle, live| {
                evaluate_watch_coverage(
                    cycle,
                    &WatchCoverageParams {
                        py_config: &live.py_config,
                        rs_config: &live.rs_config,
                        coverage_all: live.coverage_all,
                    },
                )
            },
        );
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
            Ok(Some(exit_code)) => Ok(Some(apply_watcher_client_exit(exit_code, None, None))),
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
    Ok(Some(apply_watcher_client_exit(
        reply.exit_code,
        reply.error,
        reply.output,
    )))
}

#[cfg(unix)]
fn apply_watcher_client_exit(
    exit_code: i32,
    error: Option<String>,
    output: Option<String>,
) -> i32 {
    println!("kiss test: watcher cycle complete");
    let has_report = output.as_ref().is_some_and(|s| !s.is_empty());
    if has_report {
        println!("{}", output.as_deref().unwrap_or(""));
    }
    if exit_code != 0 {
        if !has_report {
            println!("FAIL");
        }
        if let Some(err) = error {
            eprintln!("{err}");
        }
        return exit_code;
    }
    if !has_report {
        println!("PASS");
    }
    exit_code
}

/// Watcher coverage step: same gates as one-shot, but `allow_refresh: true` so a
/// load Err is repaired in this cycle instead of FAIL-looping.
pub(crate) fn evaluate_watch_coverage(
    cycle: &RunTestCmdArgs<'_>,
    cov: &WatchCoverageParams<'_>,
) -> WatchCoverageResult {
    let universe = universe_root_for_test_invocation(&cycle.invocation);
    warm_cov_caches_after_tests(
        &universe,
        cycle.lang_filter,
        cycle.ignore,
        &cycle.gate_config,
        cycle.python_extra,
    );
    let universe_s = universe.to_string_lossy().into_owned();
    let paths = [universe_s];
    let args = CovCommandArgs {
        paths: &paths,
        lang_filter: cycle.lang_filter,
        py_config: cov.py_config,
        rs_config: cov.rs_config,
        gate_config: &cycle.gate_config,
        bypass_gate: cov.coverage_all,
        ignore: cycle.ignore,
        timing: false,
        jobs: cycle.jobs,
        allow_refresh: true,
        pytest_args: cycle.python_extra,
    };
    let (cov_code, error) = run_cov_for_watch(&args);
    if crate::test_runner::consume_rust_batch_interrupted() {
        return WatchCoverageResult::interrupted();
    }
    if cov_code == 0 {
        WatchCoverageResult::ok(0)
    } else {
        WatchCoverageResult::failed(
            cov_code,
            error.unwrap_or_else(|| "coverage gate failed".to_string()),
        )
    }
}

fn run_cov_for_watch(args: &CovCommandArgs<'_>) -> (i32, Option<String>) {
    use crate::analyze::gather_files;
    use crate::bin_cli::util::merge_check_ignore_prefixes;
    use crate::test_runner::check_line_coverage::{
        RequiredCoverageLanguages, ensure_check_runtime_coverage, load_check_runtime_coverage,
        repository_root_for_universe,
    };

    if args.gate_config.test_coverage_threshold == 0
        && args.gate_config.unit_test_time_gate_disabled()
        && !args.bypass_gate
    {
        return (run_watch_cov_score(args), None);
    }

    let ignore = merge_check_ignore_prefixes(args.ignore);
    let universe = PathBuf::from(args.paths.first().map(String::as_str).unwrap_or("."));
    let repo_root = repository_root_for_universe(&universe);
    let (py_files, rs_files) = gather_files(&universe, args.lang_filter, &ignore);
    let required = RequiredCoverageLanguages {
        python: !py_files.is_empty(),
        rust: !rs_files.is_empty(),
    };
    if let Err(_load_err) = load_check_runtime_coverage(
        &repo_root,
        required,
        &ignore,
        args.gate_config,
        args.pytest_args,
    ) {
        if let Err(refresh_err) = ensure_check_runtime_coverage(
            &repo_root,
            required,
            &ignore,
            args.jobs,
            args.pytest_args,
            args.gate_config,
        ) {
            let msg = refresh_err.to_string();
            eprintln!("{msg}");
            return (1, Some(msg));
        }
        if let Err(err) = load_check_runtime_coverage(
            &repo_root,
            required,
            &ignore,
            args.gate_config,
            args.pytest_args,
        ) {
            let msg = err.to_string();
            eprintln!("{msg}");
            return (1, Some(msg));
        }
    }
    let code = run_watch_cov_score(args);
    if code == 0 {
        (0, None)
    } else {
        (code, Some("coverage gate failed".to_string()))
    }
}

fn run_watch_cov_score(args: &CovCommandArgs<'_>) -> i32 {
    let started = std::time::Instant::now();
    let code = run_cov_command(args);
    crate::test_runner::emit_stage_time("cov_score", started.elapsed());
    code
}

/// Local one-shot path after successful tests: evaluate coverage from cache only.
pub(crate) fn finish_with_coverage(args: &TestCommandArgs<'_>, test_exit: i32) -> i32 {
    let universe = universe_root_for_test_invocation(&args.invocation);
    let python_extra =
        kiss::effective_python_pytest_args(&args.test_cfg.pytest_plugins, args.extra);
    warm_cov_caches_after_tests(
        &universe,
        args.lang_filter,
        args.ignore,
        args.gate_config,
        &python_extra,
    );
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
        pytest_args: &python_extra,
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

#[cfg(test)]
#[path = "test_cmd_client_test.rs"]
mod client_tests;
