use crate::test_git::TestChangeMode;
use crate::test_runner::coverage_decision::SelectionBasis;
use crate::test_runner::runners::enumerate_workspace_rust_selectors;
use crate::test_runner::test_mode_fixtures::{
    RS_COVERING_SELECTOR, edit_rust_covered_source, with_cwd, with_locked_warm_committed_repo,
};
use crate::test_runner::{
    PlanSelectorsRequest, PlannedSelectors, SelectorRunOptions, plan_selectors, run_selectors,
};

#[test]
fn plan_selectors_commit_uses_reusable_prior_after_ordinary_rs_edit() {
    let _cwd = crate::cwd_test_lock::lock();
    with_locked_warm_committed_repo(|repo, lib| {
        edit_rust_covered_source(&lib, 2);
        let planned: PlannedSelectors = with_cwd(repo, || {
            plan_selectors(PlanSelectorsRequest {
                mode: TestChangeMode::Commit,
                main_branch_cli: None,
                base_branch_cli: None,
                ignore: &[],
                extras: crate::test_runner::language_keyed::LanguageKeyed {
                    python: &[],
                    rust: &[],
                },
                lang_filter: Some(kiss::Language::Rust),
                config_main_branch: None,
            })
        })
        .expect("plan selectors");
        let universe = enumerate_workspace_rust_selectors(repo, &[]).unwrap();
        let code = with_cwd(repo, || {
            run_selectors(
                &planned,
                SelectorRunOptions {
                    dry_run: true,
                    force_rerun: false,
                    metrics: true,
                    jobs: 1,
                    extras: crate::test_runner::language_keyed::LanguageKeyed {
                        python: &[],
                        rust: &[],
                    },
                    plan_duration: std::time::Duration::ZERO,
                    gate: kiss::GateConfig::default(),
                },
            )
        })
        .unwrap();

        assert_eq!(code, 0);
        assert!(!planned.population_required.rust);
        assert_eq!(planned.selection_basis.rust, SelectionBasis::ReusablePrior);
        assert_eq!(planned.sel.rust, vec![RS_COVERING_SELECTOR.to_string()]);
        assert!(planned.sel.rust.len() < universe.len() || universe.len() == 1);
        assert!(!planned.sel.rust.is_empty());
    });
}
