use crate::support::mv_harness::{apply_move_sequence, write_failure_artifacts};
use crate::symbol_mv_matrix::scenario_specs;

#[test]
#[ignore = "heavy corpus-style validation"]
fn heavy_mv_sequences_capture_failure_artifacts_on_semantic_breakage() {
    let mut failures: Vec<String> = Vec::new();
    for scenario in scenario_specs().into_iter().filter(|s| s.should_succeed) {
        let outcome = apply_move_sequence(&scenario, 2).expect("sequence should run");
        if !outcome.post_oracles.ok() {
            let artifact_dir =
                write_failure_artifacts(&outcome).expect("artifacts should be written");
            assert!(
                artifact_dir.join("scenario.txt").exists(),
                "failure artifact should record scenario metadata"
            );
            failures.push(format!(
                "{}: oracle failure (artifacts in {})",
                outcome.scenario_name,
                artifact_dir.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "oracle failures detected:\n{}",
        failures.join("\n")
    );
}
