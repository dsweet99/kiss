use kiss::TestSectionConfig;

use crate::bin_cli::args::TestInvocation;
use crate::test_runner::target_request::TargetRequest;
use crate::test_runner::{RunTestCmdArgs, RunTestOnceOutcome, run_test_once};

pub struct TestCommandArgs<'a> {
    pub invocation: TestInvocation,
    pub main_branch: Option<&'a str>,
    pub base_branch: Option<&'a str>,
    pub dry_run: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub ignore: &'a [String],
    pub extra: &'a [String],
    pub lang_filter: Option<kiss::Language>,
    pub test_cfg: &'a TestSectionConfig,
    pub gate_config: &'a kiss::GateConfig,
    pub language_tables: kiss::LanguageTablesPresent,
}

pub fn run_test_command(args: TestCommandArgs<'_>) -> i32 {
    run_test_command_with_runner(args, run_test_once)
}

fn request_from_test_args(args: &TestCommandArgs<'_>) -> TargetRequest {
    crate::test_runner::target_request::request_from_invocation(
        &args.invocation,
        args.main_branch,
        args.base_branch,
        args.test_cfg.main_branch.as_deref(),
        args.lang_filter,
        args.ignore,
    )
}

fn kiss_test_error_line(err: impl std::fmt::Display) -> String {
    let err = err.to_string();
    if err.starts_with("error: kiss test:") {
        err
    } else {
        format!("error: kiss test: {err}")
    }
}

fn report_kiss_test_error(err: impl std::fmt::Display) {
    eprintln!("{}", kiss_test_error_line(err));
}

fn reject_test_universe_languages(args: &TestCommandArgs<'_>) -> Result<(), i32> {
    if args.language_tables.all_present() {
        return Ok(());
    }
    let request = request_from_test_args(args);
    let paths =
        crate::test_runner::target_request::request_source_paths(&request).map_err(|err| {
            report_kiss_test_error(&err);
            1
        })?;
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
    if !args.dry_run
        && let Ok(cwd) = std::env::current_dir()
        && let Ok(repo) = crate::test_git::git_repo_root(&cwd)
    {
        kiss::test_state_lock::discard_retired_state_if_idle(&kiss::test_state_dir(&repo));
    }
    let python_extra_owned =
        kiss::effective_python_pytest_args(&args.test_cfg.pytest_plugins, args.extra);
    let request = request_from_test_args(&args);
    let run_args = RunTestCmdArgs {
        doubles: None,
        invocation: crate::test_runner::target_request::to_compat_invocation(&request),
        target_request: request,
        main_branch_cli: args.main_branch,
        base_branch_cli: args.base_branch,
        dry_run: args.dry_run,
        force_rerun: false,
        metrics: args.metrics,
        jobs: args.jobs,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            rust: args.extra,
            python: &python_extra_owned,
        },
        config_main_branch: args.test_cfg.main_branch.as_deref(),
        gate_config: args.gate_config.clone(),
    };
    if args.dry_run {
        run_dry_tests(&args, run_args, run_local)
    } else {
        run_local_tests(&args, run_args, run_local)
    }
}

fn run_dry_tests(
    args: &TestCommandArgs<'_>,
    run_args: RunTestCmdArgs<'_>,
    run_local: impl FnOnce(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
) -> i32 {
    if let Err(code) = reject_test_universe_languages(args) {
        return code;
    }
    runner_exit(run_local(run_args))
}

fn runner_exit(outcome: RunTestOnceOutcome) -> i32 {
    match outcome {
        RunTestOnceOutcome::Code(code) => code,
        RunTestOnceOutcome::Interrupted => 130,
        RunTestOnceOutcome::EngineError(_) => 1,
    }
}

#[path = "test_cmd_lock.rs"]
mod test_cmd_lock;

fn run_local_tests(
    args: &TestCommandArgs<'_>,
    run_args: RunTestCmdArgs<'_>,
    run_local: impl FnOnce(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
) -> i32 {
    if let Err(code) = reject_unresolved_targets(args) {
        return code;
    }
    let _oneshot_lock = match test_cmd_lock::take_oneshot_lock() {
        Ok(guard) => guard,
        Err(code) => return code,
    };
    if let Err(code) = reject_test_universe_languages(args) {
        return code;
    }
    runner_exit(run_local(run_args))
}

fn reject_unresolved_targets(args: &TestCommandArgs<'_>) -> Result<(), i32> {
    let request = request_from_test_args(args);
    let Some(targets) = crate::test_runner::target_request::operand_raws(&request.focus) else {
        return Ok(());
    };
    let cwd = std::env::current_dir().map_err(|e| {
        report_kiss_test_error(e);
        1
    })?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd).map_err(|e| {
        report_kiss_test_error(e);
        1
    })?;
    crate::test_runner::expand_target_operands(&repo_root, &targets, args.ignore, args.lang_filter)
        .map_err(|e| {
            report_kiss_test_error(e);
            2
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kiss_test_error_line_does_not_stack() {
        assert_eq!(
            kiss_test_error_line("pytest collection failed"),
            "error: kiss test: pytest collection failed"
        );
        let once = "error: kiss test: pytest collection failed";
        assert_eq!(kiss_test_error_line(once), once);
    }

    #[test]
    fn reject_unresolved_targets_ok_for_non_path_invocations() {
        let test_cfg = TestSectionConfig::default();
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
                metrics: false,
                jobs: 1,
                ignore: &[],
                extra: &[],
                lang_filter: None,
                test_cfg: &test_cfg,
                gate_config: &gate,
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
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::Base,
            main_branch: None,
            base_branch: Some("dev"),
            dry_run: false,
            metrics: false,
            jobs: 1,
            ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            gate_config: &gate,
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
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::Targets(vec!["z.py".into(), "a.py".into()]),
            main_branch: None,
            base_branch: None,
            dry_run: true,
            metrics: false,
            jobs: 1,
            ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            gate_config: &gate,
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
        let gate = kiss::GateConfig::default();
        crate::test_runner::test_mode_fixtures::with_cwd(tmp.path(), || {
            let args = TestCommandArgs {
                invocation: TestInvocation::Targets(vec!["missing.py".into()]),
                main_branch: None,
                base_branch: None,
                dry_run: false,
                metrics: false,
                jobs: 1,
                ignore: &[],
                extra: &[],
                lang_filter: None,
                test_cfg: &test_cfg,
                gate_config: &gate,
                language_tables: Default::default(),
            };
            assert!(reject_unresolved_targets(&args).is_err());
        });
    }

    #[test]
    fn take_oneshot_lock_acquires_in_git_repo() {
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::test_runner::test_mode_fixtures::init_git(&tmp);
        let taken = crate::test_runner::test_mode_fixtures::with_cwd(
            tmp.path(),
            test_cmd_lock::take_oneshot_lock,
        );
        assert!(taken.is_ok(), "an idle repo must grant the kiss test lock");
    }

    #[test]
    fn run_dry_tests_maps_interrupted_and_engine_error() {
        let test_cfg = TestSectionConfig::default();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::All,
            main_branch: None,
            base_branch: None,
            dry_run: true,
            metrics: false,
            jobs: 1,
            ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            gate_config: &gate,
            language_tables: kiss::LanguageTablesPresent::both(),
        };
        let mk_run_args = || RunTestCmdArgs {
            doubles: None,
            invocation: TestInvocation::All,
            target_request: crate::test_runner::target_request::workspace_request(None, &[]),
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
            config_main_branch: None,
            gate_config: gate.clone(),
        };
        assert_eq!(
            run_dry_tests(&args, mk_run_args(), |_| RunTestOnceOutcome::Interrupted),
            130
        );
        assert_eq!(
            run_dry_tests(&args, mk_run_args(), |_| RunTestOnceOutcome::EngineError(
                "x".into()
            )),
            1
        );
    }

    #[test]
    fn reject_test_universe_languages_ok_when_both_configured() {
        let test_cfg = TestSectionConfig::default();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::All,
            main_branch: None,
            base_branch: None,
            dry_run: false,
            metrics: false,
            jobs: 1,
            ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            gate_config: &gate,
            language_tables: kiss::LanguageTablesPresent::both(),
        };
        assert!(reject_test_universe_languages(&args).is_ok());
    }

    #[test]
    fn reject_test_universe_languages_scans_when_one_table_missing() {
        let test_cfg = TestSectionConfig::default();
        let gate = kiss::GateConfig::default();
        let rust_only = TestCommandArgs {
            invocation: TestInvocation::All,
            main_branch: None,
            base_branch: None,
            dry_run: false,
            metrics: false,
            jobs: 1,
            ignore: &[],
            extra: &[],
            lang_filter: Some(kiss::Language::Rust),
            test_cfg: &test_cfg,
            gate_config: &gate,
            language_tables: kiss::LanguageTablesPresent::only(kiss::Language::Rust),
        };
        // Workspace `.` has Rust sources; missing python table must still scan and accept.
        assert!(reject_test_universe_languages(&rust_only).is_ok());
        let py_only = TestCommandArgs {
            lang_filter: Some(kiss::Language::Python),
            language_tables: kiss::LanguageTablesPresent::only(kiss::Language::Python),
            ..rust_only
        };
        assert!(reject_test_universe_languages(&py_only).is_ok());
    }

    #[test]
    fn run_dry_tests_maps_code_exit() {
        let test_cfg = TestSectionConfig::default();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::All,
            main_branch: None,
            base_branch: None,
            dry_run: true,
            metrics: false,
            jobs: 1,
            ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            gate_config: &gate,
            language_tables: kiss::LanguageTablesPresent::both(),
        };
        let run_args = RunTestCmdArgs {
            doubles: None,
            invocation: TestInvocation::All,
            target_request: crate::test_runner::target_request::workspace_request(None, &[]),
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
            config_main_branch: None,
            gate_config: gate.clone(),
        };
        assert_eq!(
            run_dry_tests(&args, run_args, |_| RunTestOnceOutcome::Code(7)),
            7
        );
    }

    #[test]
    fn reject_test_universe_languages_errors_when_tables_missing() {
        let test_cfg = TestSectionConfig::default();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::All,
            main_branch: None,
            base_branch: None,
            dry_run: false,
            metrics: false,
            jobs: 1,
            ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            gate_config: &gate,
            language_tables: kiss::LanguageTablesPresent::none(),
        };
        assert_eq!(reject_test_universe_languages(&args), Err(1));
    }

    #[test]
    fn run_local_tests_returns_language_table_error() {
        let test_cfg = TestSectionConfig::default();
        let gate = kiss::GateConfig::default();
        let args = TestCommandArgs {
            invocation: TestInvocation::All,
            main_branch: None,
            base_branch: None,
            dry_run: false,
            metrics: false,
            jobs: 1,
            ignore: &[],
            extra: &[],
            lang_filter: None,
            test_cfg: &test_cfg,
            gate_config: &gate,
            language_tables: kiss::LanguageTablesPresent::none(),
        };
        let run_args = RunTestCmdArgs {
            doubles: None,
            invocation: TestInvocation::All,
            target_request: crate::test_runner::target_request::workspace_request(None, &[]),
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: false,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
            config_main_branch: None,
            gate_config: gate.clone(),
        };
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::test_runner::test_mode_fixtures::init_git(&tmp);
        std::fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
        let code = crate::test_runner::test_mode_fixtures::with_cwd(tmp.path(), || {
            run_local_tests(&args, run_args, |_| RunTestOnceOutcome::Code(0))
        });
        assert_eq!(code, 1);
    }
}

#[cfg(test)]
#[path = "test_cmd_local_test.rs"]
mod local_tests;
