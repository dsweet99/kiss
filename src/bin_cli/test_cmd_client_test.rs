use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use kiss::TestSectionConfig;

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
