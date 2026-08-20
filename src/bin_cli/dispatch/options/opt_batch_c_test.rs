use super::*;
use kiss::{Config, GateConfig, TestSectionConfig};

impl TestDispatchOptions<'_> {
    fn witness() {}
}

#[test]
fn witness_opt_batch_c() {
    TestDispatchOptions::witness();
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig::default();
    let test_cfg = TestSectionConfig::default();
    let cfg = TriConfig {
        py: &py,
        rs: &rs,
        gate: &gate,
    };
    let test = TestDispatchOptions {
        lang: Some(Language::Python),
        invocation: TestInvocation::All,
        main_branch: Some("main".into()),
        base_branch: Some("origin/main".into()),
        dry_run: true,
        force: true,
        force_bad: true,
        metrics: true,
        coverage_all: true,
        watch: false,
        jobs: Some(4),
        ignore: vec![".venv".into()],
        extra: vec!["-q".into()],
        test_cfg: &test_cfg,
        cfg: &cfg,
        reload_kissconfig: true,
        config_path: None,
    };
    assert!(test.coverage_all);
    assert_eq!(test.jobs, Some(4));
    assert!(matches!(test.invocation, TestInvocation::All));
}
