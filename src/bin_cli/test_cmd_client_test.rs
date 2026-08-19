
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use kiss::TestSectionConfig;

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
        jobs_cli: Some(1),
        ignore: &[],
        cli_ignore: &[],
        extra: &[],
        lang_filter: Some(kiss::Language::Python),
        test_cfg: &test_cfg,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        reload_kissconfig: true,
        config_path: None,
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

#[cfg(unix)]
#[test]
fn injected_client_pass_does_not_run_local_coverage() {
    let test_cfg = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 75,
        ..kiss::GateConfig::default()
    };
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
        jobs_cli: Some(1),
        ignore: &[],
        cli_ignore: &[],
        extra: &[],
        lang_filter: Some(kiss::Language::Python),
        test_cfg: &test_cfg,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        reload_kissconfig: true,
        config_path: None,
    };
    set_client_result_override_for_test(Some(Ok(Some(0))));
    let calls = AtomicUsize::new(0);
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let code = run_test_command_with(args, |_a| {
            calls.fetch_add(1, Ordering::SeqCst);
            0
        });
        assert_eq!(code, 0, "watcher client pass must exit 0 without local cov");
    });
    set_client_result_override_for_test(None);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "client path must not invoke local runner (or coverage via that path)"
    );
    assert!(
        out.contains("watcher cycle complete"),
        "stdout must report watcher cycle; got: {out:?}"
    );
    assert!(
        out.contains("PASS"),
        "stdout must report PASS; got: {out:?}"
    );
}

#[cfg(unix)]
#[test]
fn apply_watcher_client_exit_prints_output_not_bare_fail() {
    let code = apply_watcher_client_exit(
        1,
        None,
        Some(
            "✓ 1 passed · 0.1s total · 0s max pass\n\
             VIOLATION:test_coverage: codebase coverage 75% below 90% threshold\n\
             Run 'kiss rules' for more information about fixing violations."
                .into(),
        ),
    );
    assert_eq!(code, 1);
}

#[cfg(unix)]
#[test]
fn apply_watcher_client_exit_prints_fail_report() {
    let code = apply_watcher_client_exit(
        1,
        None,
        Some("✗ 0 passed · 1 failed · 0.2s total · 0s max pass\nFAIL tests/a.py::t".into()),
    );
    assert_eq!(code, 1);
}
