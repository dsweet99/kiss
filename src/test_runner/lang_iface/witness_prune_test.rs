use super::witness::{
    ExecutionWitness, WitnessScope, WitnessStatus, prune_witness_to_known_selectors,
};

#[test]
fn prune_witness_to_known_selectors_drops_removed_tests() {
    let mut w = ExecutionWitness {
        language: "rust".into(),
        scope: WitnessScope::Full,
        identity_digest: "id".into(),
        selectors: vec!["a".into(), "removed".into(), "b".into()],
        statuses: vec![
            WitnessStatus::Passed,
            WitnessStatus::Passed,
            WitnessStatus::Failed,
        ],
        durations_ns: vec![Some(1), Some(1), Some(1)],
        covered_lines: Default::default(),
        complete: true,
        generation_id: "gen-1".into(),
        raw_statuses: Vec::new(),
    };
    let known = std::collections::BTreeSet::from(["a".into(), "b".into()]);
    prune_witness_to_known_selectors(&mut w, &known);
    assert_eq!(w.selectors, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(
        w.statuses,
        vec![WitnessStatus::Passed, WitnessStatus::Failed]
    );
    assert_eq!(w.durations_ns.len(), 2);
}
