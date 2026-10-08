use super::*;

fn python_state(status: WitnessStatus, duration: Option<u64>) -> Rc<RefCell<FakeState>> {
    Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: kiss::Language::Python,
            identity_digest: "id".into(),
            selectors: vec!["a".into()],
            statuses: vec![status],
            durations_ns: vec![duration],
            complete: status == WitnessStatus::Passed,
            generation_id: "g".into(),
            raw_statuses: Vec::new(),
        }),
        run_exit_code: 0,
        ..Default::default()
    }))
}

#[test]
fn missing_durations_are_accepted_like_rust() {
    for (status, gate_off) in [
        (WitnessStatus::Unresolved, false),
        (WitnessStatus::Passed, true),
    ] {
        let state = python_state(status, None);
        let runtime = FakeRuntime {
            language: Language::Python,
            state: Rc::clone(&state),
        };
        let mut req = request(vec!["a".into()]);
        if gate_off {
            req.gate.max_unit_test_seconds.clear();
        }
        let result = ensure_runtime_cache(&req, &[&runtime]).expect("ensure");
        assert_eq!(result.exit_code, 0);
        assert!(state.borrow().run_calls.is_empty(), "{status:?}");
    }
}

#[test]
fn forced_run_with_unchanged_outcomes_still_publishes() {
    let state = python_state(WitnessStatus::Passed, Some(1_000_000));
    let runtime = FakeRuntime {
        language: Language::Python,
        state: Rc::clone(&state),
    };
    let mut req = request(vec!["a".into()]);
    req.force_selectors = vec!["a".into()];
    let result = ensure_runtime_cache(&req, &[&runtime]).expect("ensure");
    assert_eq!(result.exit_code, 0);
    assert_eq!(state.borrow().run_calls, vec![vec!["a".to_string()]]);
    assert_eq!(
        state.borrow().publish_calls,
        1,
        "executed selectors republish their records even when status and duration match"
    );
}

#[test]
fn partial_run_summary_includes_accepted_cache_hits() {
    let state = Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: kiss::Language::Python,
            identity_digest: "id".into(),
            selectors: vec!["a".into(), "b".into()],
            statuses: vec![WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: vec![Some(1_000_000), Some(1_000_000)],
            complete: true,
            generation_id: "g".into(),
            raw_statuses: Vec::new(),
        }),
        ..Default::default()
    }));
    let runtime = FakeRuntime {
        language: Language::Python,
        state: Rc::clone(&state),
    };
    let mut req = request(vec!["a".into(), "b".into()]);
    req.force_selectors = vec!["b".into()];
    let result = ensure_runtime_cache(&req, &[&runtime]).expect("ensure");
    let summary = result.by_language.python.unwrap().summary;
    assert_eq!(summary.total, 2);
    assert_eq!(summary.cache_hits, 1);
    assert_eq!(summary.cache_misses, 1);
}

#[test]
fn rust_warm_accept_still_emits_rust_identity_without_run() {
    let state = Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: kiss::Language::Rust,
            identity_digest: "id".into(),
            selectors: vec!["a".into()],
            statuses: vec![WitnessStatus::Passed],
            durations_ns: vec![Some(1)],
            complete: true,
            generation_id: "g".into(),
            raw_statuses: Vec::new(),
        }),
        ..Default::default()
    }));
    let runtime = FakeRuntime {
        language: Language::Rust,
        state: Rc::clone(&state),
    };
    let req = rust_request(vec!["a".into()]);
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let _ = ensure_runtime_cache(&req, &[&runtime]).expect("ensure");
    });
    assert!(
        out.contains("kiss test: stage rust_identity"),
        "warm accept must still emit rust_identity:\n{out}"
    );
    assert!(
        state.borrow().run_calls.is_empty(),
        "warm accept must not run"
    );
}
