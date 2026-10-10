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
        language_tables: kiss::LanguageTablesPresent::both(),
    };
    let test = TestDispatchOptions {
        lang: Some(Language::Python),
        invocation: TestInvocation::All,
        main_branch: Some("main".into()),
        base_branch: Some("origin/main".into()),
        dry_run: true,
        metrics: true,
        jobs: Some(4),
        ignore: vec![".venv".into()],
        extra: vec!["-q".into()],
        test_cfg: &test_cfg,
        cfg: &cfg,
    };
    assert_eq!(test.jobs, Some(4));
    assert!(matches!(test.invocation, TestInvocation::All));
}
