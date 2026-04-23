use crate::support::mv_harness::{apply_move_sequence, write_failure_artifacts};
use crate::symbol_mv_matrix::scenario_specs;

#[test]
fn heavy_mv_sequences_capture_failure_artifacts_on_semantic_breakage() {
    if std::env::var("KISS_HEAVY_TESTS").is_err() {
        eprintln!("skipping heavy corpus-style validation (set KISS_HEAVY_TESTS=1 to run)");
        return;
    }
    for scenario in scenario_specs().into_iter().filter(|s| s.should_succeed) {
        let outcome = apply_move_sequence(&scenario, 2).expect("sequence should run");
        if !outcome.post_oracles.ok() {
            let artifact_dir =
                write_failure_artifacts(&outcome).expect("artifacts should be written");
            assert!(
                artifact_dir.join("scenario.txt").exists(),
                "failure artifact should record scenario metadata"
            );
        }
    }
}
