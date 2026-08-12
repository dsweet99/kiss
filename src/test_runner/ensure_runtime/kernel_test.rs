//! Fake-runtime unit tests for the ensure kernel.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kiss::Language;
use rpytest_runner::TestStatus;

use super::ensure_runtime_cache;
use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, LanguageRuntime, OutcomeBatch, PublishBatch,
    WitnessScope, WitnessStatus,
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
}

struct FakeRuntime {
    language: Language,
    state: Rc<RefCell<FakeState>>,
}

impl LanguageRuntime for FakeRuntime {
    fn language(&self) -> Language {
        self.language
    }

    fn current_identity(&self, _request: &EnsureRequest) -> Result<String, String> {
        Ok("id".into())
    }

    fn load_full_witness(&self, _repo_root: &Path) -> Result<ExecutionWitness, String> {
        self.state
            .borrow()
            .witness
            .clone()
            .ok_or_else(|| "no witness".into())
    }

    fn run_selectors(
        &self,
        _request: &EnsureRequest,
        miss_set: &[String],
    ) -> Result<OutcomeBatch, String> {
        self.state.borrow_mut().run_calls.push(miss_set.to_vec());
        let mut summary = SelectorExecutionSummary {
            exit_code: self.state.borrow().run_exit_code,
            ..Default::default()
        };
        for sel in miss_set {
            let status = if self.state.borrow().run_exit_code == 0 {
                TestStatus::Passed
            } else {
                TestStatus::Failed
            };
            summary.record(SelectorExecutionRecord {
                selector: sel.clone(),
                status,
                raw_status: Some(status),
                cache_record: SelectorCacheRecord::MissStored,
                exit_code: Some(self.state.borrow().run_exit_code),
                duration: std::time::Duration::from_millis(1),
            });
        }
        Ok(OutcomeBatch {
            summary,
            selectors: miss_set.to_vec(),
            statuses: miss_set
                .iter()
                .map(|_| {
                    if self.state.borrow().run_exit_code == 0 {
                        WitnessStatus::Passed
                    } else {
                        WitnessStatus::Failed
                    }
                })
                .collect(),
            durations_ns: vec![1_000_000; miss_set.len()],
            covered_lines: BTreeMap::new(),
            publication_universe: Some(miss_set.to_vec()),
        })
    }

    fn publish_outcomes(
        &self,
        _request: &EnsureRequest,
        batch: &PublishBatch,
    ) -> Result<(), String> {
        self.state.borrow_mut().publish_calls += 1;
        let complete = batch.statuses.iter().all(|s| *s == WitnessStatus::Passed);
        self.state.borrow_mut().witness = Some(ExecutionWitness {
            language: match self.language {
                Language::Python => "python",
                Language::Rust => "rust",
            }
            .into(),
            scope: WitnessScope::Full,
            identity_digest: "id".into(),
            selectors: batch
                .publication_universe
                .clone()
                .unwrap_or_else(|| batch.selectors.clone()),
            statuses: batch.statuses.clone(),
            durations_ns: batch.durations_ns.clone(),
            covered_lines: batch.covered_lines.clone(),
            complete,
            generation_id: "gen".into(),
        });
        Ok(())
    }

    fn is_indexable_source(&self, _path: &Path, _repo_root: &Path) -> bool {
        true
    }

    fn dry_run_lines(
        &self,
        _selectors: &[String],
        _population: bool,
        _extra: &[String],
        _jobs: usize,
    ) -> Result<Vec<String>, String> {
        Ok(vec![])
    }

    fn accepted_summary(
        &self,
        _request: &EnsureRequest,
        planned: &[String],
        _witness: &ExecutionWitness,
    ) -> SelectorExecutionSummary {
        let mut summary = SelectorExecutionSummary::default();
        for sel in planned {
            summary.record(SelectorExecutionRecord {
                selector: sel.clone(),
                status: TestStatus::Passed,
                raw_status: Some(TestStatus::Passed),
                cache_record: SelectorCacheRecord::Hit,
                exit_code: Some(0),
                duration: std::time::Duration::from_millis(1),
            });
        }
        summary
    }
}

fn request(planned: Vec<String>) -> EnsureRequest {
    EnsureRequest {
        repo_root: PathBuf::from("/tmp"),
        mode: AcceptMode::All,
        lang_filter: Some(Language::Python),
        ignore: vec![],
        force: false,
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
    let result = ensure_runtime_cache(&request(vec!["a".into(), "b".into()]), &[&runtime])
        .expect("ensure");
    assert_eq!(result.exit_code, 1);
    assert_eq!(state.borrow().publish_calls, 1);
    assert_eq!(state.borrow().run_calls.len(), 1);
    let w = state.borrow().witness.clone().expect("published");
    assert_eq!(w.scope, WitnessScope::Full);
    assert!(!w.complete);
}

#[test]
fn accept_skips_run() {
    let state = Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: "python".into(),
            scope: WitnessScope::Full,
            identity_digest: "id".into(),
            selectors: vec!["a".into()],
            statuses: vec![WitnessStatus::Passed],
            durations_ns: vec![1],
            covered_lines: BTreeMap::new(),
            complete: true,
            generation_id: "g".into(),
        }),
        ..Default::default()
    }));
    let runtime = FakeRuntime {
        language: Language::Python,
        state: Rc::clone(&state),
    };
    let result = ensure_runtime_cache(&request(vec!["a".into()]), &[&runtime]).expect("ensure");
    assert_eq!(result.exit_code, 0);
    assert!(state.borrow().run_calls.is_empty());
    assert_eq!(state.borrow().publish_calls, 0);
    assert!(!result.by_language.python.unwrap().published);
}

#[test]
fn second_ensure_after_partial_failure_runs_only_problem_selectors() {
    let state = Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: "python".into(),
            scope: WitnessScope::Full,
            identity_digest: "id".into(),
            selectors: vec!["a".into(), "b".into()],
            statuses: vec![WitnessStatus::Passed, WitnessStatus::Failed],
            durations_ns: vec![1, 1],
            covered_lines: BTreeMap::new(),
            complete: false,
            generation_id: "g".into(),
        }),
        run_exit_code: 0,
        ..Default::default()
    }));
    let runtime = FakeRuntime {
        language: Language::Python,
        state: Rc::clone(&state),
    };
    let _ = ensure_runtime_cache(&request(vec!["a".into(), "b".into()]), &[&runtime])
        .expect("ensure");
    assert_eq!(state.borrow().run_calls, vec![vec!["b".to_string()]]);
}

#[test]
fn terminal_incomplete_reports_without_run_or_publish() {
    let state = Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: "python".into(),
            scope: WitnessScope::Full,
            identity_digest: "id".into(),
            selectors: vec!["a".into(), "b".into()],
            statuses: vec![WitnessStatus::Passed, WitnessStatus::Unresolved],
            durations_ns: vec![1, 5],
            covered_lines: BTreeMap::new(),
            complete: false,
            generation_id: "g".into(),
        }),
        run_exit_code: 1,
        ..Default::default()
    }));
    let runtime = FakeRuntime {
        language: Language::Python,
        state: Rc::clone(&state),
    };
    let result = ensure_runtime_cache(&request(vec!["a".into(), "b".into()]), &[&runtime])
        .expect("ensure");
    assert_ne!(result.exit_code, 0);
    assert!(state.borrow().run_calls.is_empty());
    assert_eq!(state.borrow().publish_calls, 0);
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
    assert_eq!(w.scope, WitnessScope::Full);
    assert!(w.selectors.is_empty());
}

#[test]
fn rust_accept_under_fake_runs_zero_exports_and_delta_publish() {
    // Plan unit test #7: Accept skips run; delta publish updates one selector.
    let state = Rc::new(RefCell::new(FakeState {
        witness: Some(ExecutionWitness {
            language: "rust".into(),
            scope: WitnessScope::Full,
            identity_digest: "id".into(),
            selectors: vec!["a".into(), "b".into()],
            statuses: vec![WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: vec![1, 2],
            covered_lines: BTreeMap::from([("f.rs".into(), vec![1])]),
            complete: true,
            generation_id: "g".into(),
        }),
        ..Default::default()
    }));
    let runtime = FakeRuntime {
        language: Language::Rust,
        state: Rc::clone(&state),
    };
    let mut req = request(vec![]);
    req.lang_filter = Some(Language::Rust);
    req.planned.python.clear();
    req.planned.rust = vec!["a".into(), "b".into()];
    let result = ensure_runtime_cache(&req, &[&runtime]).expect("accept");
    assert_eq!(result.exit_code, 0);
    assert!(state.borrow().run_calls.is_empty(), "Accept must not run selectors");
    assert_eq!(state.borrow().publish_calls, 0);

    // Force miss on one selector via incomplete status, then delta publish.
    state.borrow_mut().witness.as_mut().unwrap().statuses[1] = WitnessStatus::Failed;
    state.borrow_mut().witness.as_mut().unwrap().complete = false;
    state.borrow_mut().run_exit_code = 0;
    let result = ensure_runtime_cache(&req, &[&runtime]).expect("repair");
    assert_eq!(result.exit_code, 0);
    assert_eq!(state.borrow().run_calls, vec![vec!["b".to_string()]]);
    assert_eq!(state.borrow().publish_calls, 1);
}
