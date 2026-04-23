use crate::support::mv_harness::{
    ScenarioRun, apply_scenario, run_post_move_oracles, snapshot_tree,
};
use crate::support::mv_oracles::{OracleBundle, run_python_oracles, run_rust_oracles};
use crate::symbol_mv_matrix::scenario_specs;

#[test]
fn python_fixture_repos_pass_python_oracles_before_any_move() {
    for scenario in scenario_specs()
        .into_iter()
        .filter(|scenario| scenario.language == kiss::Language::Python)
    {
        let run = ScenarioRun::from_fixture(scenario).expect("fixture copy should succeed");
        let bundle: OracleBundle = run_python_oracles(&run);
        assert!(
            bundle.ok(),
            "python fixture {} should pass py_compile/import/pytest before mutation: {bundle:#?}",
            scenario.name
        );
    }
}

#[test]
fn rust_fixture_repos_pass_rust_oracles_before_any_move() {
    for scenario in scenario_specs()
        .into_iter()
        .filter(|scenario| scenario.language == kiss::Language::Rust)
    {
        let run = ScenarioRun::from_fixture(scenario).expect("fixture copy should succeed");
        let bundle: OracleBundle = run_rust_oracles(&run);
        assert!(
            bundle.ok(),
            "rust fixture {} should pass cargo check/test before mutation: {bundle:#?}",
            scenario.name
        );
    }
}

#[test]
fn deterministic_mv_scenarios_preserve_round_trip_and_locality_invariants() {
    for scenario in scenario_specs() {
        let run = apply_scenario(&scenario).expect("scenario should run");
        let post = run_post_move_oracles(&run);
        assert!(
            post.ok(),
            "post-move oracles should pass for {}: {post:#?}",
            scenario.name
        );
        assert!(
            run.locality_ok(),
            "unaffected files should remain byte-identical for {}",
            scenario.name
        );
        if scenario.checks_round_trip {
            let restored = run.apply_inverse().expect("inverse move should succeed");
            assert_eq!(
                snapshot_tree(&restored.root),
                snapshot_tree(&run.original_root),
                "round-trip should restore original tree for {}",
                scenario.name
            );
        }
    }
}
