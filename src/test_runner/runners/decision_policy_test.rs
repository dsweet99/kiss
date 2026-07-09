use std::collections::BTreeMap;

use crate::test_runner::coverage_decision::{
    CoverageFreshness, LanguagePlanner, PopulationPlan, SelectionDecision, TestSelector,
};
use crate::test_runner::python_coverage_index::{
    rebuild_python_coverage_index, write_python_population_manifest_for_args,
};
use crate::test_runner::runners::python_backer;
use crate::test_runner::runners::rust_backer::RustModule;
use crate::test_runner::rust_coverage_index::{
    rebuild_rust_coverage_index, write_rust_population_manifest_for_args,
};

struct PlannerParityCase {
    fresh_selective: PlannerPolicyState,
    stale_population: PlannerPolicyState,
    uncovered_fresh: PlannerPolicyState,
}

struct PlannerPolicyState {
    selector: TestSelector,
    freshness: CoverageFreshness,
    population: PopulationPlan,
    selection: SelectionDecision,
}

fn planner_parity_cases(
    repo_root: &std::path::Path,
    app: &std::path::Path,
    lib: &std::path::Path,
    universe: &[TestSelector; 2],
) -> Vec<PlannerParityCase> {
    rebuild_python_coverage_index(repo_root).unwrap();
    rebuild_rust_coverage_index(repo_root).unwrap();
    write_python_population_manifest_for_args(repo_root, &[universe[0].id.clone()], &[]).unwrap();
    write_rust_population_manifest_for_args(repo_root, &[universe[1].id.clone()], &[]).unwrap();

    vec![
        PlannerParityCase {
            fresh_selective: python_policy_state(repo_root, &[], &universe[0]),
            stale_population: python_policy_state_with_args(
                repo_root,
                std::slice::from_ref(&app.to_path_buf()),
                &["--stale".to_string()],
                &universe[0],
            ),
            uncovered_fresh: python_policy_state(
                repo_root,
                std::slice::from_ref(&app.to_path_buf()),
                &universe[0],
            ),
        },
        PlannerParityCase {
            fresh_selective: rust_policy_state(repo_root, &[], &universe[1]),
            stale_population: rust_policy_state_with_args(
                repo_root,
                std::slice::from_ref(&lib.to_path_buf()),
                &["--stale".to_string()],
                &universe[1],
            ),
            uncovered_fresh: rust_policy_state(
                repo_root,
                std::slice::from_ref(&lib.to_path_buf()),
                &universe[1],
            ),
        },
    ]
}

fn python_policy_state(
    repo_root: &std::path::Path,
    source_paths: &[std::path::PathBuf],
    selector: &TestSelector,
) -> PlannerPolicyState {
    python_policy_state_with_args(repo_root, source_paths, &[], selector)
}

fn python_policy_state_with_args(
    repo_root: &std::path::Path,
    source_paths: &[std::path::PathBuf],
    test_args: &[String],
    selector: &TestSelector,
) -> PlannerPolicyState {
    let module = python_backer::PythonModule::new(
        repo_root,
        source_paths,
        &BTreeMap::new(),
        test_args,
        &[],
        &[],
        &[],
    );
    let universe = vec![selector.clone()];
    PlannerPolicyState {
        selector: selector.clone(),
        freshness: <python_backer::PythonModule as LanguagePlanner>::freshness(&module, &universe)
            .unwrap(),
        population: <python_backer::PythonModule as LanguagePlanner>::population_plan(
            &module, &universe,
        ),
        selection: <python_backer::PythonModule as LanguagePlanner>::select(&module).unwrap(),
    }
}

fn rust_policy_state(
    repo_root: &std::path::Path,
    source_paths: &[std::path::PathBuf],
    selector: &TestSelector,
) -> PlannerPolicyState {
    rust_policy_state_with_args(repo_root, source_paths, &[], selector)
}

fn rust_policy_state_with_args(
    repo_root: &std::path::Path,
    source_paths: &[std::path::PathBuf],
    test_args: &[String],
    selector: &TestSelector,
) -> PlannerPolicyState {
    let module = RustModule::new(
        repo_root,
        source_paths,
        &BTreeMap::new(),
        test_args,
        &[],
        &[],
        &[],
    );
    let universe = vec![selector.clone()];
    PlannerPolicyState {
        selector: selector.clone(),
        freshness: <RustModule as LanguagePlanner>::freshness(&module, &universe).unwrap(),
        population: <RustModule as LanguagePlanner>::population_plan(&module, &universe),
        selection: <RustModule as LanguagePlanner>::select(&module).unwrap(),
    }
}

#[test]
fn concrete_language_planners_keep_policy_parity() {
    let tmp = tempfile::TempDir::new().unwrap();
    let app = tmp.path().join("app.py");
    let lib = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    std::fs::write(&app, "VALUE = 1\n").unwrap();
    std::fs::write(&lib, "pub fn value() -> i32 { 1 }\n").unwrap();
    let universe = [
        TestSelector::new(kiss::Language::Python, "tests/test_app.py::test_value"),
        TestSelector::new(kiss::Language::Rust, "crate::tests::test_value"),
    ];

    for case in planner_parity_cases(tmp.path(), &app, &lib, &universe) {
        assert_eq!(case.fresh_selective.freshness, CoverageFreshness::Fresh);
        assert!(case.fresh_selective.selection.complete);
        assert!(case.fresh_selective.selection.selectors.is_empty());
        assert_eq!(case.stale_population.freshness, CoverageFreshness::Stale);
        assert_eq!(
            case.stale_population.population.selectors,
            vec![case.stale_population.selector.clone()]
        );
        assert_eq!(case.uncovered_fresh.freshness, CoverageFreshness::Fresh);
        assert!(case.uncovered_fresh.selection.complete);
        assert!(case.uncovered_fresh.selection.selectors.is_empty());
    }
}
