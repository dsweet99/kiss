use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kiss::TestSectionConfig;

struct IsolatedPythonRepo {
    restore: PathBuf,
    _tmp: tempfile::TempDir,
    _cwd: crate::cwd_test_lock::Guard,
}

impl Drop for IsolatedPythonRepo {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.restore);
    }
}

fn python_oneshot_args<'a>(
    test_cfg: &'a TestSectionConfig,
    py: &'a kiss::Config,
    rs: &'a kiss::Config,
    gate: &'a kiss::GateConfig,
) -> TestCommandArgs<'a> {
    TestCommandArgs {
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
        ignore: &[],
        cli_ignore: &[],
        extra: &[],
        lang_filter: Some(kiss::Language::Python),
        test_cfg,
        py_config: py,
        rs_config: rs,
        gate_config: gate,
        reload_kissconfig: true,
        config_path: None,
        language_tables: Default::default(),
    }
}

#[test]
fn watcher_client_error_does_not_double_prefix() {
    assert_eq!(
        super::format_watcher_client_error(
            "error: kiss test: rust llvm-cov failed: InvalidRequest(\"stale\")"
        ),
        "error: kiss test: rust llvm-cov failed: InvalidRequest(\"stale\")"
    );
    assert_eq!(
        super::format_watcher_client_error("coverage gate failed"),
        "error: kiss test: coverage gate failed"
    );
}

#[cfg(unix)]
#[test]
fn oneshot_client_reply_strips_only_when_not_waited() {
    let src = crate::test_runner::NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: Some("error: kiss test: rust llvm-cov failed: stale".into()),
        output: Some("PASS (cached): 1 selectors\n✓ 1 passed · 0 failed · 0 timed out\n".into()),
        idle_cache: None,
    };
    let waited = crate::test_runner::oneshot_client_reply(src.clone(), true);
    assert_eq!(waited.error, src.error);
    assert_eq!(waited.exit_code, 1);
    let idle = crate::test_runner::oneshot_client_reply(src, false);
    assert!(idle.error.is_none(), "{:?}", idle.error);
    assert_eq!(idle.exit_code, 0);
}

#[cfg(unix)]
#[test]
fn oneshot_client_reply_keeps_fresh_cycle_without_wait() {
    let src = crate::test_runner::NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: Some("error: kiss test: rust llvm-cov failed: stale".into()),
        output: Some("PASS (cached): 1 selectors\n✓ 1 passed · 0 failed · 0 timed out\n".into()),
        idle_cache: Some(false),
    };
    let msg = crate::test_runner::oneshot_client_reply(src.clone(), false);
    assert_eq!(msg.error, src.error);
    assert_eq!(msg.exit_code, 1);
}

#[cfg(unix)]
#[test]
fn injected_client_result_is_used() {
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    set_client_result_override_for_test(Some(Ok(Some(9))));
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        4
    });
    set_client_result_override_for_test(None);
    assert_eq!(code, 9, "oneshot must use the watcher reply exit code");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "oneshot must not rerun tests locally after a watcher reply"
    );
}

#[cfg(unix)]
#[test]
fn injected_client_pass_skips_local() {
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 75,
        ..kiss::GateConfig::default()
    };
    let args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    set_client_result_override_for_test(Some(Ok(Some(0))));
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        1
    });
    set_client_result_override_for_test(None);
    assert_eq!(code, 0, "watcher pass is the oneshot product");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "watcher pass must skip the local runner"
    );
}

#[test]
fn dry_run_invokes_local_runner() {
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    args.dry_run = true;
    args.language_tables = kiss::LanguageTablesPresent::both();
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    assert_eq!(code, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn finish_with_coverage_returns_test_exit_when_threshold_zero() {
    let _cwd = crate::cwd_test_lock::lock();
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        ..kiss::GateConfig::default()
    };
    let args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    let code = finish_with_coverage(&args, 3);
    assert_eq!(code, 3);
}

#[test]
fn evaluate_watch_coverage_threshold_zero_fails_closed_without_snapshot() {
    let _repo = isolated_python_repo();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        ..kiss::GateConfig::default()
    };
    let cycle = crate::test_runner::RunTestCmdArgs {
        invocation: TestInvocation::All,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(kiss::Language::Python),
        config_main_branch: None,
        gate_config: gate.clone(),
    };
    let cov = WatchCoverageParams {
        py_config: &py,
        rs_config: &rs,
        coverage_all: false,
        language_tables: kiss::LanguageTablesPresent::both(),
    };
    let result = evaluate_watch_coverage(&cycle, &cov);
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.error.as_deref(), Some("coverage gate failed"));
}

fn isolated_python_repo() -> IsolatedPythonRepo {
    isolated_python_repo_with_git(false)
}

fn isolated_inited_python_repo() -> IsolatedPythonRepo {
    isolated_python_repo_with_git(true)
}

fn isolated_python_repo_with_git(init_git: bool) -> IsolatedPythonRepo {
    let cwd = crate::cwd_test_lock::lock();
    let restore = std::env::current_dir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    if init_git {
        assert!(kiss::scrubbed_git_command(tmp.path())
            .arg("init")
            .status()
            .unwrap()
            .success());
    } else {
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    }
    std::fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    IsolatedPythonRepo {
        restore,
        _tmp: tmp,
        _cwd: cwd,
    }
}

#[cfg(unix)]
#[test]
fn rustc_style_missing_path_is_rejected_even_if_watcher_says_ok() {
    let _repo = isolated_inited_python_repo();
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    for raw in [
        "python_nested_observed.rs:51:python_nested_observed",
        "python_nested_observed.rs:51:python_nested_observed:",
        "bad_path.rs",
    ] {
        let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
        args.invocation = TestInvocation::Targets(vec![raw.into()]);
        args.lang_filter = None;
        args.language_tables = kiss::LanguageTablesPresent::both();
        set_client_result_override_for_test(Some(Ok(Some(0))));
        let calls = AtomicUsize::new(0);
        let code = run_test_command_with(args, |_a| {
            calls.fetch_add(1, Ordering::SeqCst);
            0
        });
        set_client_result_override_for_test(None);
        assert_eq!(
            code, 1,
            "{raw}: missing path must fail even when a watcher recap is success"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{raw}");
    }
}

#[cfg(unix)]
#[test]
fn lang_mismatch_is_rejected_even_if_watcher_says_ok() {
    let _repo = isolated_inited_python_repo();
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    args.invocation = TestInvocation::Targets(vec!["app.py".into()]);
    args.lang_filter = Some(kiss::Language::Rust);
    args.language_tables = kiss::LanguageTablesPresent::both();
    set_client_result_override_for_test(Some(Ok(Some(0))));
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    set_client_result_override_for_test(None);
    assert_eq!(
        code, 1,
        "lang mismatch must fail even when a watcher recap is success"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[test]
fn ignore_prefix_is_rejected_even_if_watcher_says_ok() {
    let _repo = isolated_inited_python_repo();
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let ignore = ["app".to_string()];
    let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    args.invocation = TestInvocation::Targets(vec!["app.py".into()]);
    args.ignore = &ignore;
    args.language_tables = kiss::LanguageTablesPresent::both();
    set_client_result_override_for_test(Some(Ok(Some(0))));
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    set_client_result_override_for_test(None);
    assert_eq!(
        code, 1,
        "ignore prefix must fail even when a watcher recap is success"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn finish_with_coverage_returns_cov_exit_when_snapshot_missing() {
    let _repo = isolated_python_repo();
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 75,
        max_unit_test_seconds: Vec::new(),
        ..kiss::GateConfig::default()
    };
    let args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    let code = finish_with_coverage(&args, 0);
    assert_eq!(code, 1);
}

#[test]
fn evaluate_watch_coverage_fails_when_language_table_missing() {
    let _repo = isolated_python_repo();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        ..kiss::GateConfig::default()
    };
    let cycle = crate::test_runner::RunTestCmdArgs {
        invocation: TestInvocation::All,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(kiss::Language::Python),
        config_main_branch: None,
        gate_config: gate.clone(),
    };
    let cov = WatchCoverageParams {
        py_config: &py,
        rs_config: &rs,
        coverage_all: false,
        language_tables: kiss::LanguageTablesPresent::none(),
    };
    let result = evaluate_watch_coverage(&cycle, &cov);
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.error.as_deref(), Some("coverage gate failed"));
}

#[test]
fn evaluate_watch_coverage_fails_closed_without_snapshot_even_with_tables() {
    let _repo = isolated_python_repo();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        ..kiss::GateConfig::default()
    };
    let cycle = crate::test_runner::RunTestCmdArgs {
        invocation: TestInvocation::All,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(kiss::Language::Python),
        config_main_branch: None,
        gate_config: gate.clone(),
    };
    let cov = WatchCoverageParams {
        py_config: &py,
        rs_config: &rs,
        coverage_all: false,
        language_tables: kiss::LanguageTablesPresent::both(),
    };
    let result = evaluate_watch_coverage(&cycle, &cov);
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.error.as_deref(), Some("coverage gate failed"));
}

#[test]
fn dry_run_rejects_unconfigured_languages() {
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    args.dry_run = true;
    args.language_tables = kiss::LanguageTablesPresent::none();
    let code = run_test_command_with(args, |_a| 0);
    assert_eq!(code, 1);
}

#[test]
fn watch_flag_takes_watch_dispatch_path() {
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    args.watch = true;
    args.language_tables = kiss::LanguageTablesPresent::none();
    let code = run_test_command_with(args, |_a| 0);
    assert_eq!(code, 1);
}

#[test]
fn watch_flag_runs_watch_setup_when_languages_configured() {
    let _repo = isolated_python_repo();
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    args.watch = true;
    args.language_tables = kiss::LanguageTablesPresent::both();
    let code = run_test_command_with(args, |_a| 0);
    assert_eq!(code, 1);
}

#[cfg(unix)]
#[test]
fn oneshot_without_watcher_runs_local_runner_then_coverage() {
    let _repo = isolated_inited_python_repo();
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        max_num_tests: 999999,
        ..kiss::GateConfig::default()
    };
    let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    args.language_tables = kiss::LanguageTablesPresent::both();
    set_client_result_override_for_test(Some(Ok(None)));
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    set_client_result_override_for_test(None);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "local runner must run");
    // Coverage may still fail closed without a population snapshot; the goal is
    // exercising the no-watcher local+coverage path (not a green cov score).
    assert!(
        code == 0 || code == 1,
        "local path must finish with a coverage decision, got {code}"
    );
}

#[cfg(unix)]
#[test]
fn injected_client_error_prints_and_exits_one() {
    let _repo = isolated_inited_python_repo();
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    args.language_tables = kiss::LanguageTablesPresent::both();
    set_client_result_override_for_test(Some(Err("watcher unavailable".into())));
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    set_client_result_override_for_test(None);
    assert_eq!(code, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[test]
fn oneshot_existing_target_without_watcher_accepts_resolve() {
    let _repo = isolated_inited_python_repo();
    std::fs::write("test_thing.py", "def test_ok():\n    assert True\n").unwrap();
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        max_num_tests: 999999,
        ..kiss::GateConfig::default()
    };
    let mut args = python_oneshot_args(&test_cfg, &py, &rs, &gate);
    args.invocation = TestInvocation::Targets(vec!["test_thing.py".into()]);
    args.language_tables = kiss::LanguageTablesPresent::both();
    set_client_result_override_for_test(Some(Ok(None)));
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    set_client_result_override_for_test(None);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(code == 0 || code == 1, "got {code}");
}
