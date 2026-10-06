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
        jobs: 1,
        ignore: &[],
        extra: &[],
        lang_filter: Some(kiss::Language::Python),
        test_cfg,
        gate_config: gate,
        language_tables: Default::default(),
    }
}

#[test]
fn dry_run_invokes_local_runner() {
    let test_cfg = TestSectionConfig::default();
    let gate = kiss::GateConfig::default();
    let mut args = python_oneshot_args(&test_cfg, &gate);
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
    let _repo = isolated_inited_python_repo();
    let test_cfg = TestSectionConfig::default();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        ..kiss::GateConfig::default()
    };
    let args = python_oneshot_args(&test_cfg, &gate);
    let code = finish_with_coverage(&args, 3);
    assert_eq!(code, 3);
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
        assert!(
            kiss::scrubbed_git_command(tmp.path())
                .arg("init")
                .status()
                .unwrap()
                .success()
        );
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

#[test]
fn rustc_style_missing_path_is_rejected_before_running() {
    let _repo = isolated_inited_python_repo();
    let test_cfg = TestSectionConfig::default();
    let gate = kiss::GateConfig::default();
    for raw in [
        "python_nested_observed.rs:51:python_nested_observed",
        "python_nested_observed.rs:51:python_nested_observed:",
        "bad_path.rs",
    ] {
        let mut args = python_oneshot_args(&test_cfg, &gate);
        args.invocation = TestInvocation::Targets(vec![raw.into()]);
        args.lang_filter = None;
        args.language_tables = kiss::LanguageTablesPresent::both();
        let calls = AtomicUsize::new(0);
        let code = run_test_command_with(args, |_a| {
            calls.fetch_add(1, Ordering::SeqCst);
            0
        });
        assert_eq!(
            code, 2,
            "{raw}: missing path must fail"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{raw}");
    }
}

#[test]
fn lang_mismatch_is_rejected_before_running() {
    let _repo = isolated_inited_python_repo();
    let test_cfg = TestSectionConfig::default();
    let gate = kiss::GateConfig::default();
    let mut args = python_oneshot_args(&test_cfg, &gate);
    args.invocation = TestInvocation::Targets(vec!["app.py".into()]);
    args.lang_filter = Some(kiss::Language::Rust);
    args.language_tables = kiss::LanguageTablesPresent::both();
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    assert_eq!(
        code, 2,
        "lang mismatch must fail"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn ignore_prefix_is_rejected_before_running() {
    let _repo = isolated_inited_python_repo();
    let test_cfg = TestSectionConfig::default();
    let gate = kiss::GateConfig::default();
    let ignore = ["app".to_string()];
    let mut args = python_oneshot_args(&test_cfg, &gate);
    args.invocation = TestInvocation::Targets(vec!["app.py".into()]);
    args.ignore = &ignore;
    args.language_tables = kiss::LanguageTablesPresent::both();
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    assert_eq!(
        code, 2,
        "ignore prefix must fail"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn finish_with_coverage_returns_cov_exit_when_snapshot_missing() {
    let _repo = isolated_python_repo();
    let test_cfg = TestSectionConfig::default();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 75,
        max_unit_test_seconds: Vec::new(),
        ..kiss::GateConfig::default()
    };
    let args = python_oneshot_args(&test_cfg, &gate);
    let code = finish_with_coverage(&args, 0);
    assert_eq!(code, 1);
}

#[test]
fn dry_run_rejects_unconfigured_languages() {
    let test_cfg = TestSectionConfig::default();
    let gate = kiss::GateConfig::default();
    let mut args = python_oneshot_args(&test_cfg, &gate);
    args.dry_run = true;
    args.language_tables = kiss::LanguageTablesPresent::none();
    let code = run_test_command_with(args, |_a| 0);
    assert_eq!(code, 1);
}

#[test]
fn oneshot_runs_local_runner_then_coverage() {
    let _repo = isolated_inited_python_repo();
    std::fs::write("test_app.py", "def test_ok():\n    assert True\n").unwrap();
    let test_cfg = TestSectionConfig::default();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        max_num_tests: 999999,
        ..kiss::GateConfig::default()
    };
    let mut args = python_oneshot_args(&test_cfg, &gate);
    args.language_tables = kiss::LanguageTablesPresent::both();
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1, "local runner must run");
    // Coverage may still fail closed without a population snapshot; the goal is
    // exercising the local+coverage path (not a green cov score).
    assert!(
        code == 0 || code == 1,
        "local path must finish with a coverage decision, got {code}"
    );
}

#[test]
fn oneshot_existing_target_accepts_resolve() {
    let _repo = isolated_inited_python_repo();
    std::fs::write("test_thing.py", "def test_ok():\n    assert True\n").unwrap();
    let test_cfg = TestSectionConfig::default();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        max_num_tests: 999999,
        ..kiss::GateConfig::default()
    };
    let mut args = python_oneshot_args(&test_cfg, &gate);
    args.invocation = TestInvocation::Targets(vec!["test_thing.py".into()]);
    args.language_tables = kiss::LanguageTablesPresent::both();
    let calls = AtomicUsize::new(0);
    let code = run_test_command_with(args, |_a| {
        calls.fetch_add(1, Ordering::SeqCst);
        0
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(code == 0 || code == 1, "got {code}");
}
