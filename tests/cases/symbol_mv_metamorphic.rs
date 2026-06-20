use crate::support::mv_harness::{
    ScenarioRun, apply_scenario, run_post_move_oracles, snapshot_tree,
};
use crate::support::mv_oracles::{OracleBundle, run_python_oracles, run_rust_oracles};
use crate::symbol_mv_matrix::{ScenarioSpec, scenario_specs};

fn scenario_named(name: &str) -> ScenarioSpec {
    scenario_specs()
        .into_iter()
        .find(|scenario| scenario.name == name)
        .unwrap_or_else(|| panic!("missing scenario {name}"))
}

fn assert_python_fixture_oracles(scenario: ScenarioSpec) {
    let run = ScenarioRun::from_fixture(scenario).expect("fixture copy should succeed");
    let bundle: OracleBundle = run_python_oracles(&run);
    assert!(
        bundle.ok(),
        "python fixture {} should pass py_compile/import/pytest before mutation: {bundle:#?}",
        scenario.name
    );
}

fn assert_rust_fixture_oracles(scenario: ScenarioSpec) {
    let run = ScenarioRun::from_fixture(scenario).expect("fixture copy should succeed");
    let bundle: OracleBundle = run_rust_oracles(&run);
    assert!(
        bundle.ok(),
        "rust fixture {} should pass cargo check/test before mutation: {bundle:#?}",
        scenario.name
    );
}

fn assert_mv_scenario_invariants(scenario: ScenarioSpec) {
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

#[test]
fn python_method_rename_fixture_passes_oracles_before_move() {
    assert_python_fixture_oracles(scenario_named("python_method_rename"));
}

#[test]
fn python_move_only_fixture_passes_oracles_before_move() {
    assert_python_fixture_oracles(scenario_named("python_move_only"));
}

#[test]
fn python_move_and_rename_fixture_passes_oracles_before_move() {
    assert_python_fixture_oracles(scenario_named("python_move_and_rename"));
}

#[test]
fn rust_method_rename_fixture_passes_oracles_before_move() {
    assert_rust_fixture_oracles(scenario_named("rust_method_rename"));
}

#[test]
fn rust_move_only_fixture_passes_oracles_before_move() {
    assert_rust_fixture_oracles(scenario_named("rust_move_only"));
}

#[test]
fn rust_move_and_rename_fixture_passes_oracles_before_move() {
    assert_rust_fixture_oracles(scenario_named("rust_move_and_rename"));
}

#[test]
fn python_method_rename_mv_preserves_invariants() {
    assert_mv_scenario_invariants(scenario_named("python_method_rename"));
}

#[test]
fn python_move_only_mv_preserves_invariants() {
    assert_mv_scenario_invariants(scenario_named("python_move_only"));
}

#[test]
fn python_move_and_rename_mv_preserves_invariants() {
    assert_mv_scenario_invariants(scenario_named("python_move_and_rename"));
}

#[test]
fn rust_method_rename_mv_preserves_invariants() {
    assert_mv_scenario_invariants(scenario_named("rust_method_rename"));
}

#[test]
fn rust_move_only_mv_preserves_invariants() {
    assert_mv_scenario_invariants(scenario_named("rust_move_only"));
}

#[test]
fn rust_move_and_rename_mv_preserves_invariants() {
    assert_mv_scenario_invariants(scenario_named("rust_move_and_rename"));
}
