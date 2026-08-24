#[test]
fn run_test_emits_planning_heartbeat_before_plan_work() {
    let tmp = tempfile::tempdir().unwrap();
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let code = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
            invocation: crate::bin_cli::args::TestInvocation::Commit,
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
            lang_filter: None,
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
        });
        assert_eq!(code, 1);
    });
    std::env::set_current_dir(old).unwrap();
    assert!(
        out.contains("kiss test: Planning ..."),
        "expected early planning heartbeat before plan failure, got {out:?}"
    );
}
