use super::*;

#[test]
fn identity_drift_with_unchanged_outcomes_still_publishes() {

    let state = Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: "python".into(),
            scope: WitnessScope::Full,
            identity_digest: "old-id".into(),
            selectors: vec!["a".into()],
            statuses: vec![WitnessStatus::Passed],
            durations_ns: vec![Some(1_000_000)],
            covered_lines: BTreeMap::new(),
            complete: true,
            generation_id: "g".into(),
        }),
        run_exit_code: 0,
        identity: Some("new-id".into()),
        ..Default::default()
    }));
    let runtime = FakeRuntime {
        language: Language::Python,
        state: Rc::clone(&state),
    };
    let result = ensure_runtime_cache(&request(vec!["a".into()]), &[&runtime]).expect("ensure");
    assert_eq!(result.exit_code, 0);
    assert_eq!(state.borrow().run_calls, vec![vec!["a".to_string()]]);
    assert_eq!(
        state.borrow().publish_calls, 1,
        "must publish even when outcomes match prior, so cov sees current fingerprint"
    );
    let w = state.borrow().witness.clone().expect("published");
    assert_eq!(w.identity_digest, "new-id");
}
