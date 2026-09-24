use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;

use kiss::TestSectionConfig;

use crate::bin_cli::args::TestInvocation;
use crate::test_runner::{
    RunTestCmdArgs, RunTestOnceOutcome, WatchCoverageParams, kiss_report_from_ensure_outcome,
    run_test_once, run_test_watch,
};

#[path = "test_cmd_coverage.rs"]
mod coverage;
#[cfg(test)]
pub(crate) use coverage::coverage_result_from_exit;
pub(crate) use coverage::evaluate_watch_coverage;
#[cfg(test)]
pub(crate) use coverage::finish_with_coverage;
use coverage::request_from_test_args;

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

#[cfg(unix)]
fn take_client_result_override() -> Option<Result<Option<i32>, String>> {
    CLIENT_RESULT_OVERRIDE.with(Cell::take)
}

pub fn run_test_command(args: TestCommandArgs<'_>) -> i32 {
    run_test_command_with_runner(args, run_test_once)
}

fn reject_test_universe_languages(args: &TestCommandArgs<'_>) -> Result<(), i32> {
    if args.language_tables.python && args.language_tables.rust {
        return Ok(());
    }
    let request = request_from_test_args(args);
    let paths = crate::test_runner::target_request::coverage_paths_for_request(&request).map_err(
        |err| {
            eprintln!("error: kiss test: {err}");
            1
        },
    )?;
    let (py_files, rs_files) = kiss::gather_files_by_lang(&paths, args.lang_filter, args.ignore);
    crate::bin_cli::util::reject_unconfigured_languages(&py_files, &rs_files, args.language_tables)
}

#[cfg(test)]
pub(crate) fn run_test_command_with(
    args: TestCommandArgs<'_>,
    run_local: impl FnOnce(RunTestCmdArgs<'_>) -> i32,
) -> i32 {
    run_test_command_with_runner(args, |a| RunTestOnceOutcome::Code(run_local(a)))
}

fn run_test_command_with_runner(
    args: TestCommandArgs<'_>,
    run_local: impl FnOnce(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
) -> i32 {
    let python_extra_owned =
        kiss::effective_python_pytest_args(&args.test_cfg.pytest_plugins, args.extra);
    let request = request_from_test_args(&args);
    let run_args = RunTestCmdArgs {
        invocation: crate::test_runner::target_request::to_compat_invocation(&request),
        target_request: request,
        main_branch_cli: args.main_branch,
        base_branch_cli: args.base_branch,
        dry_run: args.dry_run,
        force_rerun: false,
        force_bad: args.retry_bad,
        metrics: args.metrics,
        coverage_all: args.coverage_all,
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
                    coverage_all: live.coverage_all || live.nudge_coverage_all,
                    language_tables: live.language_tables,
                },
            )
        },
    )
}

fn run_dry_tests(
    args: &TestCommandArgs<'_>,
    run_args: RunTestCmdArgs<'_>,
    run_local: impl FnOnce(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
) -> i32 {
    if let Err(code) = reject_test_universe_languages(args) {
        return code;
    }
    match run_local(run_args) {
        RunTestOnceOutcome::Code(code) => code,
        RunTestOnceOutcome::Interrupted => 130,
        RunTestOnceOutcome::EngineError(_) => 1,
    }
}

#[cfg(unix)]
#[path = "test_cmd_lock.rs"]
mod test_cmd_lock;

fn run_local_tests_after_client(
    args: &TestCommandArgs<'_>,
    run_args: RunTestCmdArgs<'_>,
    run_local: impl FnOnce(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
) -> i32 {
    if let Err(code) = reject_unresolved_targets(args) {
        return code;
    }
    #[cfg(unix)]
    let _oneshot_lock = match test_cmd_lock::take_oneshot_lock(args) {
        Ok(guard) => guard,
        Err(code) => return code,
    };
    #[cfg(not(unix))]
    if let Some(code) = wait_out_live_watcher(args) {
        return code;
    }
    if let Err(code) = reject_test_universe_languages(args) {
        return code;
    }
    let mut run_local = Some(run_local);
    let repo = std::env::current_dir()
        .ok()
        .and_then(|cwd| crate::test_git::git_repo_root(&cwd).ok());
    kiss_report_from_ensure_outcome(crate::test_runner::target_request::ensure_target_report(
        repo.as_deref(),
        &run_args,
        true,
        false,
        |a| run_local.take().expect("kiss test runner")(a),
    ))
    .exit_code
}

fn reject_unresolved_targets(args: &TestCommandArgs<'_>) -> Result<(), i32> {
    let request = request_from_test_args(args);
    let Some(targets) = crate::test_runner::target_request::operand_raws(&request.focus) else {
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
    crate::test_runner::expand_target_operands(&repo_root, &targets, args.ignore, args.lang_filter)
        .map_err(|e| {
            eprintln!("error: kiss test: {e}");
            1
        })?;
    Ok(())
}

#[cfg(not(unix))]
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
    let repo = std::env::current_dir()
        .ok()
        .and_then(|cwd| crate::test_git::git_repo_root(&cwd).ok());
    let (runner, configuration) = repo
        .as_deref()
        .map(|root| {
            (
                crate::test_runner::target_request::runner_identity(root),
                crate::test_runner::target_request::configuration_generation(root),
            )
        })
        .unwrap_or_default();
    crate::test_runner::NudgeRequestMsg {
        force: false,
        force_bad: args.retry_bad,
        metrics: args.metrics,
        extra: args.extra.to_vec(),
        python_extra: kiss::effective_python_pytest_args(&args.test_cfg.pytest_plugins, args.extra),
        target_request: request_from_test_args(args),
        coverage_all: args.coverage_all,
        runner,
        configuration,
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
    let reply = match nudge_watcher_with_retry_on_wait(
        &repo_root,
        &session,
        &nudge_request_from_test_args(args),
        &mut || {
            if !printed_waiting {
                printed_waiting = true;
                println!("kiss test: waiting for watcher (pid {pid})");
            }
        },
    ) {
        Ok(reply) => reply,
        Err(err) if err.contains("cannot connect") || err.contains("Broken pipe") => {
            crate::test_runner::reclaim_stale_watch_session(&repo_root);
            return Ok(None);
        }
        Err(err) => return Err(err),
    };
    let reply = crate::test_runner::oneshot_client_reply(reply, printed_waiting);
    if let Some(output) = reply.output.as_deref()
        && !output.is_empty()
    {
        print!("{output}");
        if !output.ends_with('\n') {
            println!();
        }
    }
    if let Some(error) = reply.error.as_deref() {
        eprintln!("{}", format_watcher_client_error(error));
    }
    Ok(Some(reply.exit_code))
}

fn format_watcher_client_error(error: &str) -> String {
    if error.starts_with("error: ") {
        error.to_string()
    } else {
        format!("error: kiss test: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_result_from_exit_success_failure_and_interrupt() {
        let _ = crate::test_runner::consume_rust_batch_interrupted();
        let ok = coverage_result_from_exit(0);
        assert_eq!(ok.exit_code, 0);
        assert!(ok.error.is_none());
        assert!(!ok.interrupted);

        let bad = coverage_result_from_exit(4);
        assert_eq!(bad.exit_code, 4);
        assert_eq!(bad.error.as_deref(), Some("coverage gate failed"));

        crate::test_runner::note_rust_batch_interrupted();
        let interrupted = coverage_result_from_exit(0);
        assert_eq!(interrupted.exit_code, 130);
        assert!(interrupted.interrupted);
        assert!(interrupted.error.is_none());
    }

    #[test]
    fn reject_unresolved_targets_ok_for_non_path_invocations() {
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        for invocation in [
            TestInvocation::All,
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
            assert!(
                reject_unresolved_targets(&args).is_ok(),
                "invocation={:?}",
                args.invocation
            );
        }
    }

    #[test]
    fn request_from_test_args_keeps_explicit_base() {
        use crate::test_runner::target_request::{GitFocus, TargetFocus, request_from_focus};
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::Base,
            main_branch: None,
            base_branch: Some("dev"),
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
        let request = request_from_test_args(&args);
        assert_eq!(
            request,
            request_from_focus(
                TargetFocus::Git(GitFocus::ExplicitBase {
                    branch: "dev".into(),
                }),
                None,
                &[],
            )
        );
        assert!(matches!(
            request.focus,
            TargetFocus::Git(GitFocus::ExplicitBase { branch }) if branch == "dev"
        ));
    }

    #[test]
    fn request_from_focus_explicit_base_sorts_ignore_prefixes() {
        use crate::test_runner::target_request::{GitFocus, TargetFocus, request_from_focus};
        let request = request_from_focus(
            TargetFocus::Git(GitFocus::ExplicitBase {
                branch: "dev".into(),
            }),
            None,
            &["z".into(), "a".into(), "a".into()],
        );
        assert!(matches!(
            request.focus,
            TargetFocus::Git(GitFocus::ExplicitBase { branch }) if branch == "dev"
        ));
        assert_eq!(request.ignore, vec!["a".to_string(), "z".to_string()]);
    }

    #[test]
    fn run_test_command_syncs_invocation_from_request() {
        use crate::test_runner::target_request::to_compat_invocation;
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::Targets(vec!["z.py".into(), "a.py".into()]),
            main_branch: None,
            base_branch: None,
            dry_run: true,
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
        let expected = to_compat_invocation(&request_from_test_args(&args));
        let mut seen = None;
        let code = run_test_command_with(args, |run| {
            seen = Some(run.invocation.clone());
            0
        });
        assert_eq!(code, 0);
        assert_eq!(seen, Some(expected));
        assert_eq!(
            seen,
            Some(TestInvocation::Targets(vec!["a.py".into(), "z.py".into()]))
        );
    }

    #[test]
    fn reject_unresolved_targets_rejects_missing_operand() {
        let tmp = tempfile::TempDir::new().unwrap();
        crate::test_runner::test_mode_fixtures::init_git(&tmp);
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        crate::test_runner::test_mode_fixtures::with_cwd(tmp.path(), || {
            let args = TestCommandArgs {
                invocation: TestInvocation::Targets(vec!["missing.py".into()]),
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
            assert!(reject_unresolved_targets(&args).is_err());
        });
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
        assert!(!msg.runner.is_empty());
        assert!(!msg.configuration.is_empty());
        assert!(msg.target_request.language().is_none());
        assert!(msg.target_request.ignore.is_empty());
        assert!(msg.extra.is_empty());
        assert_eq!(msg.target_request, request_from_test_args(&args));
        assert_eq!(
            crate::test_runner::target_request::operand_raws(&msg.target_request.focus),
            Some(vec![
                "tests/fast/analysis/test_gantt.py::test_one".to_string()
            ])
        );
        let all_args = TestCommandArgs {
            invocation: TestInvocation::All,
            ..args
        };
        let all_msg = nudge_request_from_test_args(&all_args);
        assert!(!all_msg.force);
        assert_eq!(all_msg.target_request, request_from_test_args(&all_args));
    }

    #[cfg(unix)]
    #[test]
    fn nudge_request_forwards_lang_ignore_and_extra() {
        let test_cfg = TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let ignore = ["test_".to_string()];
        let extra = ["-k".to_string(), "does_not_match".to_string()];
        let args = TestCommandArgs {
            invocation: TestInvocation::All,
            main_branch: None,
            base_branch: None,
            dry_run: false,
            retry_bad: false,
            metrics: false,
            coverage_all: false,
            watch: false,
            jobs: 1,
            jobs_cli: Some(1),
            ignore: &ignore,
            cli_ignore: &ignore,
            extra: &extra,
            lang_filter: Some(kiss::Language::Rust),
            test_cfg: &test_cfg,
            py_config: &py,
            rs_config: &rs,
            gate_config: &gate,
            reload_kissconfig: false,
            config_path: None,
            language_tables: Default::default(),
        };
        let msg = nudge_request_from_test_args(&args);
        assert_eq!(msg.lang_filter(), Some(kiss::Language::Rust));
        assert_eq!(msg.target_request.ignore, ignore);
        assert_eq!(msg.target_request, request_from_test_args(&args));
        assert_eq!(msg.extra, extra);
        assert_eq!(msg.python_extra, extra);
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
        assert_eq!(msg.target_request, request_from_test_args(&args));
        assert_eq!(
            crate::test_runner::target_request::operand_raws(&msg.target_request.focus),
            Some(vec![
                "tests/test_trio.py::test_first".to_string(),
                "tests/test_trio.py::test_third".to_string()
            ])
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
            assert_eq!(
                msg.target_request,
                request_from_test_args(&args),
                "commit/base/main must be forwarded as the pin"
            );
        }
    }
}

#[cfg(test)]
#[path = "test_cmd_client_test.rs"]
mod client_tests;
