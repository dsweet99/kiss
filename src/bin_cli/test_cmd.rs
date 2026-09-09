use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;

use kiss::TestSectionConfig;

use crate::bin_cli::args::TestInvocation;
use crate::bin_cli::cov_cmd::{CovCommandArgs, run_cov_command_impl};
use crate::test_runner::{
    RunTestCmdArgs, RunTestOnceOutcome, WatchCoverageParams, WatchCoverageResult,
    KISS_TEST_ALLOW_REFRESH, run_kiss_test_report, run_test, run_test_watch,
};

pub struct TestCommandArgs<'a> {
    pub invocation: TestInvocation,
    pub main_branch: Option<&'a str>,
    pub base_branch: Option<&'a str>,
    pub dry_run: bool,
    pub retry_bad: bool,
    pub metrics: bool,
    pub coverage_all: bool,
    pub watch: bool,
    pub jobs: usize,
    pub jobs_cli: Option<usize>,
    pub ignore: &'a [String],
    pub cli_ignore: &'a [String],
    pub extra: &'a [String],
    pub lang_filter: Option<kiss::Language>,
    pub test_cfg: &'a TestSectionConfig,
    pub py_config: &'a kiss::Config,
    pub rs_config: &'a kiss::Config,
    pub gate_config: &'a kiss::GateConfig,
    pub reload_kissconfig: bool,
    pub config_path: Option<&'a PathBuf>,
    pub language_tables: kiss::LanguageTablesPresent,
}

thread_local! {
    static CLIENT_RESULT_OVERRIDE: Cell<Option<Result<Option<i32>, String>>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_client_result_override_for_test(value: Option<Result<Option<i32>, String>>) {
    CLIENT_RESULT_OVERRIDE.with(|c| c.set(value));
}

pub fn run_test_command(args: TestCommandArgs<'_>) -> i32 {
    run_test_command_with(args, run_test)
}

fn reject_test_universe_languages(args: &TestCommandArgs<'_>) -> Result<(), i32> {
    if args.language_tables.python && args.language_tables.rust {
        return Ok(());
    }
    let universe = universe_root_for_test_invocation(&args.invocation);
    let path = universe.to_string_lossy().into_owned();
    let (py_files, rs_files) =
        kiss::gather_files_by_lang(std::slice::from_ref(&path), args.lang_filter, args.ignore);
    crate::bin_cli::util::reject_unconfigured_languages(&py_files, &rs_files, args.language_tables)
}

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
        force_rerun: false,
        force_bad: args.retry_bad,
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
        run_watch_tests(&args, run_args)
    } else if args.dry_run {
        run_dry_tests(&args, run_args, run_local)
    } else {
        run_local_tests_after_client(&args, run_args, run_local)
    }
}

fn run_watch_tests(args: &TestCommandArgs<'_>, run_args: RunTestCmdArgs<'_>) -> i32 {
    if let Err(code) = reject_test_universe_languages(args) {
        return code;
    }
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
    run_test_watch(
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
                    language_tables: live.language_tables,
                },
            )
        },
    )
}

fn run_dry_tests(
    args: &TestCommandArgs<'_>,
    run_args: RunTestCmdArgs<'_>,
    run_local: impl FnOnce(RunTestCmdArgs<'_>) -> i32,
) -> i32 {
    if let Err(code) = reject_test_universe_languages(args) {
        return code;
    }
    run_local(run_args)
}

fn run_local_tests_after_client(
    args: &TestCommandArgs<'_>,
    run_args: RunTestCmdArgs<'_>,
    run_local: impl FnOnce(RunTestCmdArgs<'_>) -> i32,
) -> i32 {
    if let Err(code) = reject_unresolved_targets(args) {
        return code;
    }
    if let Some(code) = wait_out_live_watcher(args) {
        return code;
    }
    if let Err(code) = reject_test_universe_languages(args) {
        return code;
    }
    let mut run_local = Some(run_local);
    let report = run_kiss_test_report(
        run_args,
        |a| RunTestOnceOutcome::Code(run_local.take().expect("kiss test runner")(a)),
        |_| coverage_after_kiss_test(args),
    );
    report.exit_code
}

fn reject_unresolved_targets(args: &TestCommandArgs<'_>) -> Result<(), i32> {
    let TestInvocation::Targets(targets) = &args.invocation else {
        return Ok(());
    };
    let cwd = std::env::current_dir().map_err(|e| {
        eprintln!("error: kiss test: {e}");
        1
    })?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd).map_err(|e| {
        eprintln!("error: kiss test: {e}");
        1
    })?;
    crate::test_runner::expand_target_operands(&repo_root, targets, args.ignore, args.lang_filter)
        .map_err(|e| {
            eprintln!("error: kiss test: {e}");
            1
        })?;
    Ok(())
}

fn wait_out_live_watcher(args: &TestCommandArgs<'_>) -> Option<i32> {
    #[cfg(unix)]
    let result = match try_wait_out_live_watcher(args) {
        Ok(Some(code)) => Some(code),
        Ok(None) => None,
        Err(e) => {
            eprintln!("error: kiss test: {e}");
            Some(1)
        }
    };
    #[cfg(not(unix))]
    let result = {
        let _ = args;
        None
    };
    result
}

#[cfg(unix)]
fn nudge_request_from_test_args(args: &TestCommandArgs<'_>) -> crate::test_runner::NudgeRequestMsg {
    crate::test_runner::NudgeRequestMsg {
        force: false,
        force_bad: args.retry_bad,
        metrics: args.metrics,
        invocation: crate::test_runner::NudgeInvocation::from_test(&args.invocation),
        targets: match &args.invocation {
            TestInvocation::Targets(targets) => targets.clone(),
            _ => Vec::new(),
        },
    }
}

#[cfg(unix)]
fn try_wait_out_live_watcher(args: &TestCommandArgs<'_>) -> Result<Option<i32>, String> {
    if let Some(overridden) = CLIENT_RESULT_OVERRIDE.with(Cell::take) {
        return overridden;
    }

    use crate::test_runner::{nudge_watcher_with_retry_on_wait, probe_live_watcher};

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd)?;
    let Some(session) = probe_live_watcher(&repo_root)? else {
        return Ok(None);
    };
    let pid = session.pid;
    let mut printed_waiting = false;
    let reply = nudge_watcher_with_retry_on_wait(
        &repo_root,
        &session,
        &nudge_request_from_test_args(args),
        &mut || {
            if !printed_waiting {
                printed_waiting = true;
                println!("kiss test: waiting for watcher (pid {pid})");
            }
        },
    )?;
    if let Some(output) = reply.output.as_deref()
        && !output.is_empty()
    {
        print!("{output}");
        if !output.ends_with('\n') {
            println!();
        }
    }
    if let Some(error) = reply.error.as_deref() {
        eprintln!("error: kiss test: {error}");
    }
    Ok(Some(reply.exit_code))
}

struct AfterTestCoverage<'a> {
    invocation: &'a TestInvocation,
    lang_filter: Option<kiss::Language>,
    ignore: &'a [String],
    gate_config: &'a kiss::GateConfig,
    py_config: &'a kiss::Config,
    rs_config: &'a kiss::Config,
    coverage_all: bool,
    jobs: usize,
    allow_refresh: bool,
    pytest_args: &'a [String],
    language_tables: kiss::LanguageTablesPresent,
}

fn run_after_test_coverage(p: AfterTestCoverage<'_>) -> i32 {
    let universe = universe_root_for_test_invocation(p.invocation);
    let universe_s = universe.to_string_lossy().into_owned();
    let paths = [universe_s];
    let started = std::time::Instant::now();
    let code = run_cov_command_impl(
        &CovCommandArgs {
            paths: &paths,
            lang_filter: p.lang_filter,
            py_config: p.py_config,
            rs_config: p.rs_config,
            gate_config: p.gate_config,
            bypass_gate: p.coverage_all,
            ignore: p.ignore,
            timing: false,
            jobs: p.jobs,
            allow_refresh: p.allow_refresh,
            pytest_args: p.pytest_args,
            language_tables: p.language_tables,
        },
        false,
    );
    crate::test_runner::emit_stage_time("cov_score", started.elapsed());
    code
}

pub(crate) fn evaluate_watch_coverage(
    cycle: &RunTestCmdArgs<'_>,
    cov: &WatchCoverageParams<'_>,
) -> WatchCoverageResult {
    let cov_code = run_after_test_coverage(AfterTestCoverage {
        invocation: &cycle.invocation,
        lang_filter: cycle.lang_filter,
        ignore: cycle.ignore,
        gate_config: &cycle.gate_config,
        py_config: cov.py_config,
        rs_config: cov.rs_config,
        coverage_all: cov.coverage_all,
        jobs: cycle.jobs,
        allow_refresh: KISS_TEST_ALLOW_REFRESH,
        pytest_args: cycle.python_extra,
        language_tables: cov.language_tables,
    });
    coverage_result_from_exit(cov_code)
}

fn coverage_after_kiss_test(args: &TestCommandArgs<'_>) -> WatchCoverageResult {
    let python_extra =
        kiss::effective_python_pytest_args(&args.test_cfg.pytest_plugins, args.extra);
    coverage_result_from_exit(run_after_test_coverage(AfterTestCoverage {
        invocation: &args.invocation,
        lang_filter: args.lang_filter,
        ignore: args.ignore,
        gate_config: args.gate_config,
        py_config: args.py_config,
        rs_config: args.rs_config,
        coverage_all: args.coverage_all,
        jobs: args.jobs,
        allow_refresh: KISS_TEST_ALLOW_REFRESH,
        pytest_args: &python_extra,
        language_tables: args.language_tables,
    }))
}

fn coverage_result_from_exit(cov_code: i32) -> WatchCoverageResult {
    if crate::test_runner::consume_rust_batch_interrupted() {
        return WatchCoverageResult::interrupted();
    }
    if cov_code == 0 {
        WatchCoverageResult::ok(0)
    } else {
        WatchCoverageResult::failed(cov_code, "coverage gate failed")
    }
}

#[cfg(test)]
pub(crate) fn finish_with_coverage(args: &TestCommandArgs<'_>, test_exit: i32) -> i32 {
    let python_extra =
        kiss::effective_python_pytest_args(&args.test_cfg.pytest_plugins, args.extra);
    let cov_code = run_after_test_coverage(AfterTestCoverage {
        invocation: &args.invocation,
        lang_filter: args.lang_filter,
        ignore: args.ignore,
        gate_config: args.gate_config,
        py_config: args.py_config,
        rs_config: args.rs_config,
        coverage_all: args.coverage_all,
        jobs: args.jobs,
        allow_refresh: KISS_TEST_ALLOW_REFRESH,
        pytest_args: &python_extra,
        language_tables: args.language_tables,
    });
    if test_exit != 0 { test_exit } else { cov_code }
}

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
        assert_eq!(
            universe_root_for_test_invocation(&TestInvocation::Targets(vec![".".into()])),
            PathBuf::from(".")
        );
        assert_eq!(
            universe_root_for_test_invocation(&TestInvocation::Targets(vec!["./".into()])),
            PathBuf::from(".")
        );
    }

    #[cfg(unix)]
    #[test]
    fn nudge_request_forwards_selected_targets_without_cli_force() {
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::Targets(vec![
                "tests/fast/analysis/test_gantt.py::test_one".into(),
            ]),
            main_branch: None,
            base_branch: None,
            dry_run: false,
            retry_bad: false,
            metrics: false,
            coverage_all: false,
            watch: false,
            jobs: 1,
            jobs_cli: Some(1),
            ignore: &[],
            cli_ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            py_config: &py,
            rs_config: &rs,
            gate_config: &gate,
            reload_kissconfig: false,
            config_path: None,
            language_tables: Default::default(),
        };
        let msg = nudge_request_from_test_args(&args);
        assert!(!msg.force);
        assert!(!msg.force_bad);
        assert_eq!(
            msg.targets,
            vec!["tests/fast/analysis/test_gantt.py::test_one".to_string()]
        );
        let all_args = TestCommandArgs {
            invocation: TestInvocation::All,
            ..args
        };
        let all_msg = nudge_request_from_test_args(&all_args);
        assert!(!all_msg.force);
        assert!(all_msg.targets.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn nudge_request_forwards_retry_bad() {
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::All,
            main_branch: None,
            base_branch: None,
            dry_run: false,
            retry_bad: true,
            metrics: false,
            coverage_all: false,
            watch: false,
            jobs: 1,
            jobs_cli: Some(1),
            ignore: &[],
            cli_ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            py_config: &py,
            rs_config: &rs,
            gate_config: &gate,
            reload_kissconfig: false,
            config_path: None,
            language_tables: Default::default(),
        };
        let msg = nudge_request_from_test_args(&args);
        assert!(!msg.force);
        assert!(msg.force_bad);
    }

    #[cfg(unix)]
    #[test]
    fn nudge_request_forwards_two_force_targets() {
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::Targets(vec![
                "tests/test_trio.py::test_first".into(),
                "tests/test_trio.py::test_third".into(),
            ]),
            main_branch: None,
            base_branch: None,
            dry_run: false,
            retry_bad: false,
            metrics: false,
            coverage_all: false,
            watch: false,
            jobs: 1,
            jobs_cli: Some(1),
            ignore: &[],
            cli_ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            py_config: &py,
            rs_config: &rs,
            gate_config: &gate,
            reload_kissconfig: false,
            config_path: None,
            language_tables: Default::default(),
        };
        let msg = nudge_request_from_test_args(&args);
        assert!(!msg.force);
        assert_eq!(
            msg.targets,
            vec![
                "tests/test_trio.py::test_first".to_string(),
                "tests/test_trio.py::test_third".to_string()
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn nudge_request_commit_base_main_are_forwarded() {
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        for invocation in [
            TestInvocation::Commit,
            TestInvocation::Base,
            TestInvocation::Main,
        ] {
            let args = TestCommandArgs {
                invocation,
                main_branch: None,
                base_branch: None,
                dry_run: false,
                retry_bad: false,
                metrics: false,
                coverage_all: false,
                watch: false,
                jobs: 1,
                jobs_cli: Some(1),
                ignore: &[],
                cli_ignore: &[],
                extra: &[],
                lang_filter: None,
                test_cfg: &test_cfg,
                py_config: &py,
                rs_config: &rs,
                gate_config: &gate,
                reload_kissconfig: false,
                config_path: None,
                language_tables: Default::default(),
            };
            let msg = nudge_request_from_test_args(&args);
            assert!(!msg.force, "invocation={:?}", args.invocation);
            assert!(
                msg.targets.is_empty(),
                "reserved actions must not invent path targets; invocation={:?}",
                args.invocation
            );
            assert_eq!(
                msg.invocation,
                crate::test_runner::NudgeInvocation::from_test(&args.invocation),
                "commit/base/main must be forwarded to the watcher"
            );
        }
    }
}

#[cfg(test)]
#[path = "test_cmd_client_test.rs"]
mod client_tests;
