use tempfile::TempDir;

use crate::test_git::TestChangeMode;
use crate::test_runner::coverage_decision::RustSelectionBasis;
use crate::test_runner::runners::enumerate_workspace_rust_selectors;
use crate::test_runner::test_mode_fixtures::{
    RS_COVERING_SELECTOR, edit_rust_covered_source, warm_committed_rust_demo,
};
use crate::test_runner::{
    PlanSelectorsRequest, PlannedSelectors, SelectorRunOptions, plan_selectors, run_selectors,
};

#[test]
fn plan_selectors_commit_uses_reusable_prior_after_ordinary_rs_edit() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let lib = warm_committed_rust_demo(&tmp);
    edit_rust_covered_source(&lib, 2);

    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let planned: PlannedSelectors = plan_selectors(PlanSelectorsRequest {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        ignore: &[],
        extra: &[],
        python_extra: &[],
        lang_filter: Some(kiss::Language::Rust),
        config_main_branch: None,
    })
    .expect("plan selectors");
    let universe = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap();
    let code = run_selectors(
        &planned,
        SelectorRunOptions {
            dry_run: true,
            force_rerun: false,
metrics: true,
            jobs: 1,
            extra: &[],
            python_extra: &[],
            plan_duration: std::time::Duration::ZERO,
        gate: kiss::GateConfig::default()
        },
    )
    .unwrap();
    std::env::set_current_dir(orig).unwrap();

    assert_eq!(code, 0);
    assert!(!planned.population_required.rust);
    assert_eq!(
        planned.rust_selection_basis,
        RustSelectionBasis::ReusablePrior
    );
    assert_eq!(planned.sel.rust, vec![RS_COVERING_SELECTOR.to_string()]);
    assert!(planned.sel.rust.len() < universe.len() || universe.len() == 1);
    assert!(!planned.sel.rust.is_empty());
}
