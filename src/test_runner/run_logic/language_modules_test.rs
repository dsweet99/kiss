use super::*;
use crate::test_runner::{PlannedSelectors, SelectorRunOptions};
use std::path::PathBuf;
use std::time::Duration;

fn planned() -> PlannedSelectors {
    PlannedSelectors {
        repo_root: PathBuf::from("."),
        py_sel: vec!["tests/test_app.py::test_ok".to_string()],
        rs_sel: vec!["crate::tests::test_ok".to_string()],
        python_population_required: false,
        python_population_selectors: vec!["tests/test_app.py::test_population".to_string()],
        rust_source_paths: Vec::new(),
        rust_source_population_paths: Vec::new(),
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        ignore: Vec::new(),
    }
}

fn options() -> SelectorRunOptions<'static> {
    SelectorRunOptions {
        dry_run: true,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        plan_duration: Duration::ZERO,
    }
}

#[test]
fn python_module_policy_reads_python_selectors() {
    let mut planned = planned();
    planned.python_population_required = true;
    let options = options();
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    assert!(python_population_required(&ctx));
    assert_eq!(
        python_population_selectors(&ctx).unwrap(),
        vec!["tests/test_app.py::test_population".to_string()]
    );
    assert_eq!(
        python_selective_selectors(&ctx),
        vec!["tests/test_app.py::test_ok".to_string()]
    );
}

#[test]
fn rust_module_policy_reads_rust_selectors() {
    let mut planned = planned();
    planned.rust_source_population_paths = vec![PathBuf::from("src/lib.rs")];
    let options = options();
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    assert!(rust_population_required(&ctx));
    assert_eq!(
        rust_selective_selectors(&ctx),
        vec!["crate::tests::test_ok".to_string()]
    );
}
