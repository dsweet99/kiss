use crate::test_runner::lang_iface::{WitnessScope, WitnessStatus};
use crate::test_runner::python_coverage_index::generation::{
    PinnedPythonGeneration, PythonExecutionIdentity, PythonPopulationPlan, SelectorTimingRecord,
    TimingCacheDisposition,
};

use super::witness_view::{python_identity_digest, python_witness_from_pinned};

fn sample_pinned(complete: bool) -> PinnedPythonGeneration {
    PinnedPythonGeneration {
        generation_id: "gen-1".into(),
        plan: PythonPopulationPlan {
            base_identity: PythonExecutionIdentity {
                schema_version: "rslip-python-generation-v1".into(),
                runner_semantics_version: "python-rslip-runner-v1".into(),
                collector_semantics_version: "python-pytest-collector-v1".into(),
                source_root: ".".into(),
                interpreter_identity: "py".into(),
                python_version: "3.12".into(),
                pytest_version: "8".into(),
                plugin_identities: vec![],
                pytest_args: vec![],
                pytest_config_digest: "x".into(),
                kissconfig_test_digest: "y".into(),
                coverage_env_digest: "z".into(),
                env: Default::default(),
                input_fingerprint: "inp".into(),
                selector_discovery_version: "v".into(),
                cache_schema_version: "c".into(),
            },
            selectors: vec!["t.py::test_a".into()],
        },
        complete,
        coverage: Default::default(),
        timings: vec![SelectorTimingRecord {
            selector: "t.py::test_a".into(),
            raw_status: "passed".into(),
            effective_status: "passed".into(),
            duration_ns: Some(12),
            cache_disposition: TimingCacheDisposition::Hit,
            reason: None,
        }],
        line_index: Default::default(),
        selector_coverage: Default::default(),
    }
}

#[test]
fn python_witness_from_complete_generation_is_full() {
    let pinned = sample_pinned(true);
    let witness = python_witness_from_pinned(&pinned);
    assert_eq!(witness.scope, WitnessScope::Full);
    assert!(witness.complete);
    assert_eq!(witness.statuses, vec![WitnessStatus::Passed]);
    assert_eq!(python_identity_digest(&pinned), witness.identity_digest);
}

#[test]
fn incomplete_generation_is_full_incomplete() {
    let pinned = sample_pinned(false);
    let witness = python_witness_from_pinned(&pinned);
    assert_eq!(witness.scope, WitnessScope::Full);
    assert!(!witness.complete);
}
