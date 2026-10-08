use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use kiss::Language;
use kiss::rpytest_runner::TestStatus;

use super::ensure_runtime_cache;
use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, LanguageRuntime, Listing, OutcomeBatch,
    WitnessStatus,
};
use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};

#[derive(Default)]
struct FakeState {
    witness: Option<ExecutionWitness>,
    run_calls: Vec<Vec<String>>,
    publish_calls: usize,
    run_exit_code: i32,
    identity: Option<String>,
}

struct FakeRuntime {
    language: Language,
    state: Rc<RefCell<FakeState>>,
}

impl crate::test_runner::test_selection::SupportedLanguage for FakeRuntime {
    fn language(&self) -> Language {
        self.language
    }
}

impl LanguageRuntime for FakeRuntime {
    fn list(&self, request: &EnsureRequest) -> Result<Listing, String> {
        Ok(Listing {
            ids: request.planned_for(self.language).to_vec(),
            identity: self
                .state
                .borrow()
                .identity
                .clone()
                .unwrap_or_else(|| "id".into()),
            record_identity: "records".into(),
        })
    }

    fn deps(
        &self,
        _request: &EnsureRequest,
        _row: &kiss::test_records::TestRecord,
    ) -> Option<BTreeMap<String, String>> {
        None
    }

    fn seeded_rows(&self, _request: &EnsureRequest) -> Option<ExecutionWitness> {
        self.state.borrow().witness.clone()
    }

    fn run(
        &self,
        _request: &EnsureRequest,
        miss_set: &[String],
        on_result: &mut dyn FnMut(SelectorExecutionRecord),
    ) -> Result<OutcomeBatch, String> {
        let batch = self.run_selectors(miss_set, on_result);
        self.store_records(&batch);
        Ok(batch)
    }
}

impl FakeRuntime {
    fn run_selectors(
        &self,
        miss_set: &[String],
        on_result: &mut dyn FnMut(SelectorExecutionRecord),
    ) -> OutcomeBatch {
        if !miss_set.is_empty() {
            self.state.borrow_mut().run_calls.push(miss_set.to_vec());
        }
        let summary = SelectorExecutionSummary {
            exit_code: self.state.borrow().run_exit_code,
            ..Default::default()
        };
        for sel in miss_set {
            let status = if self.state.borrow().run_exit_code == 0 {
                TestStatus::Passed
            } else {
                TestStatus::Failed
            };
            on_result(SelectorExecutionRecord {
                selector: sel.clone(),
                status,
                raw_status: Some(status),
                cache_record: SelectorCacheRecord::MissStored,
                exit_code: Some(self.state.borrow().run_exit_code),
                duration: std::time::Duration::from_millis(1),
            });
            if self.state.borrow().run_exit_code == 0 {
                crate::test_runner::emit_test_progress(&format!("PASS: {sel}"));
            }
        }
        OutcomeBatch {
            summary,
            selectors: miss_set.to_vec(),
        }
    }

    fn store_records(&self, batch: &OutcomeBatch) {
        self.state.borrow_mut().publish_calls += 1;
        let status = if batch.summary.exit_code == 0 {
            WitnessStatus::Passed
        } else {
            WitnessStatus::Failed
        };
        let statuses = vec![status; batch.selectors.len()];
        let complete = statuses.iter().all(|s| *s == WitnessStatus::Passed);
        let identity_digest = self
            .state
            .borrow()
            .identity
            .clone()
            .unwrap_or_else(|| "id".into());
        self.state.borrow_mut().witness = Some(ExecutionWitness {
            language: self.language,
            identity_digest,
            selectors: batch.selectors.clone(),
            durations_ns: vec![Some(1_000_000); statuses.len()],
            statuses,
            complete,
            generation_id: "gen".into(),
            raw_statuses: Vec::new(),
        });
    }
}

fn request(planned: Vec<String>) -> EnsureRequest {
    EnsureRequest {
        repo_root: PathBuf::from("/tmp"),
        mode: AcceptMode::All,
        lang_filter: Some(Language::Python),
        ignore: vec![],
        force: false,
        force_selectors: Vec::new(),
        jobs: 1,
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: planned,
            rust: vec![],
        },
    }
}

fn rust_request(planned: Vec<String>) -> EnsureRequest {
    let mut req = request(vec![]);
    req.lang_filter = Some(Language::Rust);
    req.planned.python.clear();
    req.planned.rust = planned;
    req
}

#[test]
fn miss_runs_and_publishes_even_when_exit_nonzero() {
    let state = Rc::new(RefCell::new(FakeState {
        run_exit_code: 1,
        ..Default::default()
    }));
    let runtime = FakeRuntime {
        language: Language::Python,
        state: Rc::clone(&state),
    };
    let result =
        ensure_runtime_cache(&request(vec!["a".into(), "b".into()]), &[&runtime]).expect("ensure");
    assert_eq!(result.exit_code, 1);
    assert_eq!(state.borrow().publish_calls, 1);
    assert_eq!(state.borrow().run_calls.len(), 1);
    let w = state.borrow().witness.clone().expect("published");
    assert!(!w.complete);
}

fn witness(language: Language, statuses: &[WitnessStatus], complete: bool) -> ExecutionWitness {
    ExecutionWitness {
        language,
        identity_digest: "id".into(),
        selectors: (0..statuses.len()).map(|i| format!("t{i}")).collect(),
        statuses: statuses.to_vec(),
        durations_ns: vec![Some(1); statuses.len()],
        complete,
        generation_id: "g".into(),
        raw_statuses: Vec::new(),
    }
}

/// The selectors handed to the runner for a stored witness, in each language.
fn run_calls_per_language(
    statuses: &[WitnessStatus],
    complete: bool,
) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let planned: Vec<String> = (0..statuses.len()).map(|i| format!("t{i}")).collect();
    let run = |language: Language, req: EnsureRequest| {
        let state = Rc::new(RefCell::new(FakeState {
            witness: Some(witness(language, statuses, complete)),
            ..Default::default()
        }));
        let runtime = FakeRuntime {
            language,
            state: Rc::clone(&state),
        };
        let _ = ensure_runtime_cache(&req, &[&runtime]).expect("ensure");
        state.borrow().run_calls.clone()
    };
    (
        run(Language::Python, request(planned.clone())),
        run(Language::Rust, rust_request(planned)),
    )
}

#[test]
fn python_and_rust_accept_stored_outcomes_alike() {
    for (statuses, complete) in [
        (vec![WitnessStatus::Passed], true),
        (vec![WitnessStatus::Passed, WitnessStatus::Failed], false),
        (
            vec![WitnessStatus::Passed, WitnessStatus::Unresolved],
            false,
        ),
    ] {
        let (python, rust) = run_calls_per_language(&statuses, complete);
        assert_eq!(python, rust, "{statuses:?}");
    }
}

#[test]
fn python_cached_pass_does_not_rerun() {
    let (python, _) = run_calls_per_language(&[WitnessStatus::Passed], true);
    assert!(python.is_empty(), "{python:?}");
}

#[test]
fn empty_all_mode_publishes_empty_full_without_run() {
    let state = Rc::new(RefCell::new(FakeState::default()));
    let runtime = FakeRuntime {
        language: Language::Python,
        state: Rc::clone(&state),
    };
    let mut req = request(vec![]);
    req.mode = AcceptMode::All;
    let result = ensure_runtime_cache(&req, &[&runtime]).expect("ensure");
    assert_eq!(result.exit_code, 0);
    assert!(state.borrow().run_calls.is_empty());
    assert_eq!(state.borrow().publish_calls, 1);
    let w = state.borrow().witness.clone().expect("published");
    assert!(w.selectors.is_empty());
}

#[test]
fn rust_accept_under_fake_runs_zero_exports_and_delta_publish() {
    let state = Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: kiss::Language::Rust,
            identity_digest: "id".into(),
            selectors: vec!["a".into(), "b".into()],
            statuses: vec![WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: vec![Some(1), Some(2)],
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
    let req = rust_request(vec!["a".into(), "b".into()]);
    let result = ensure_runtime_cache(&req, &[&runtime]).expect("accept");
    assert_eq!(result.exit_code, 0);
    assert!(
        state.borrow().run_calls.is_empty(),
        "Accept must not run selectors"
    );
    assert_eq!(state.borrow().publish_calls, 0);

    state.borrow_mut().witness.as_mut().unwrap().statuses[1] = WitnessStatus::Failed;
    state.borrow_mut().witness.as_mut().unwrap().complete = false;
    state.borrow_mut().run_exit_code = 0;
    let _ = ensure_runtime_cache(&req, &[&runtime]).expect("repair");
    assert!(
        state.borrow().run_calls.is_empty(),
        "a cached FAIL with unchanged identity must not rerun"
    );
    let observed = kiss::subprocess_observer::subprocess_observer_snapshot();
    assert_eq!(observed.nextest_invocations, 0);
}

fn rust_selecting_miss_recaps_witness_complement(mode: AcceptMode) {
    let state = Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: kiss::Language::Rust,
            identity_digest: "rs:old:g:s".into(),
            selectors: vec!["b".into()],
            statuses: vec![WitnessStatus::Passed],
            durations_ns: vec![Some(2)],
            complete: true,
            generation_id: "g".into(),
            raw_statuses: Vec::new(),
        }),
        identity: Some("rs:new:g:s".into()),
        run_exit_code: 0,
        ..Default::default()
    }));
    let runtime = FakeRuntime {
        language: Language::Rust,
        state: Rc::clone(&state),
    };
    let mut req = rust_request(vec!["a".into()]);
    req.mode = mode;
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let result = ensure_runtime_cache(&req, &[&runtime]).expect("ensure");
        let rust = result.by_language.rust.expect("rust result");
        assert_eq!(
            rust.summary.total, 2,
            "selecting miss of a must still recap witness b (mode {mode:?})"
        );
        assert_eq!(rust.summary.cache_hits, 1);
    });
    assert_eq!(state.borrow().run_calls, vec![vec!["a".to_string()]]);
    assert!(
        !out.contains("PASS b"),
        "a cached rust PASS that did not run has no line of its own:\n{out}"
    );
}

#[test]
fn rust_selecting_subset_miss_recaps_witness_complement() {
    rust_selecting_miss_recaps_witness_complement(AcceptMode::Subset);
}

#[test]
fn rust_selecting_all_mode_subset_recaps_witness_complement() {
    rust_selecting_miss_recaps_witness_complement(AcceptMode::All);
}

#[test]
fn rust_miss_emits_rust_identity_before_pass() {
    let state = Rc::new(RefCell::new(FakeState::default()));
    let runtime = FakeRuntime {
        language: Language::Rust,
        state: Rc::clone(&state),
    };
    let req = rust_request(vec!["a".into()]);
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let _ = ensure_runtime_cache(&req, &[&runtime]).expect("ensure");
    });
    let identity_idx = out
        .find("kiss test: stage rust_identity")
        .expect("rust_identity stage line");
    let pass_idx = out.find("PASS:").expect("PASS line from run_selectors");
    assert!(
        identity_idx < pass_idx,
        "rust_identity must precede PASS on stdout:\n{out}"
    );
    assert_eq!(state.borrow().run_calls.len(), 1);
}

#[test]
fn python_miss_does_not_emit_rust_identity() {
    let state = Rc::new(RefCell::new(FakeState::default()));
    let runtime = FakeRuntime {
        language: Language::Python,
        state: Rc::clone(&state),
    };
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let _ = ensure_runtime_cache(&request(vec!["a".into()]), &[&runtime]).expect("ensure");
    });
    assert!(
        !out.contains("kiss test: stage rust_identity"),
        "Python ensure must not emit rust_identity:\n{out}"
    );
}

#[path = "kernel_identity_publish_test.rs"]
mod identity_publish;
