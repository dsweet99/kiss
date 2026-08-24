use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kiss::TestSectionConfig;

struct IsolatedPythonRepo {
    restore: PathBuf,
    _tmp: tempfile::TempDir,
    _cwd: std::sync::MutexGuard<'static, ()>,
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
        force: false,
        force_bad: false,
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

#[cfg(unix)]
#[test]
fn injected_client_result_still_runs_local_test() {
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
    assert_eq!(
        code, 4,
        "local runner exit is the product, not the watcher reply"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "waiting on W must still call the local test runner"
    );
}

#[cfg(unix)]
#[test]
fn injected_client_pass_still_runs_local() {
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
    assert_eq!(code, 1, "watcher pass must not skip the local runner");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "waiting on W must not skip run_local"
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
fn evaluate_watch_coverage_threshold_zero_returns_ok() {
    let _cwd = crate::cwd_test_lock::lock();
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
    assert_eq!(result.exit_code, 0);
}

fn isolated_python_repo() -> IsolatedPythonRepo {
    let cwd = crate::cwd_test_lock::lock();
    let restore = std::env::current_dir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    IsolatedPythonRepo {
        restore,
        _tmp: tmp,
        _cwd: cwd,
    }
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
fn evaluate_watch_coverage_uses_params_tables_when_cwd_has_no_kissconfig() {
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
    assert_eq!(result.exit_code, 0);
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
