use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

pub(crate) use crate::rpytest_runner::forkserver_controller_runtime::ForkserverController;
#[cfg(test)]
pub(crate) use crate::rpytest_runner::forkserver_wire::{WireArtifact, WireRequest, WireResponse};

use crate::rpytest_runner::{PytestRunError, PytestRunOutcome, PytestRunRequest};

#[derive(Clone, Copy, Debug, Default)]
pub struct ForkserverPytestRunner;

impl ForkserverPytestRunner {
    pub fn new() -> Self {
        Self
    }

    pub fn run_one(&self, req: PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> {
        let python = req.python.clone();
        let bootstrap = req.bootstrap.clone();
        let mut controller = ForkserverController::start(&python, &bootstrap)?;
        controller.run(req)
    }

    pub fn run_many(
        &self,
        reqs: Vec<PytestRunRequest>,
    ) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
        let max_jobs = if reqs.is_empty() { 1 } else { reqs.len() };
        self.run_many_bounded(reqs, max_jobs)
    }

    pub fn run_many_bounded(
        &self,
        reqs: Vec<PytestRunRequest>,
        max_jobs: usize,
    ) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
        crate::rpytest_runner::runner::collect_bounded_results(reqs, max_jobs, |reqs, max_jobs, on_complete| {
            self.run_many_bounded_with_on_complete(reqs, max_jobs, on_complete);
        })
    }

    pub fn run_many_bounded_with_on_complete(
        &self,
        reqs: Vec<PytestRunRequest>,
        max_jobs: usize,
        mut on_complete: impl FnMut(usize, Result<PytestRunOutcome, PytestRunError>),
    ) {
        assert!(max_jobs > 0, "max_jobs must be greater than zero");
        let len = reqs.len();
        if len == 0 {
            return;
        }

        let queue = Arc::new(Mutex::new(reqs.into_iter().enumerate().collect()));
        let (tx, rx) = mpsc::channel();
        for _ in 0..max_jobs.min(len) {
            spawn_forkserver_worker(Arc::clone(&queue), tx.clone());
        }
        drop(tx);

        for (index, result) in rx {
            on_complete(index, result);
        }
    }
}

pub fn forkserver_pytest_runner() -> crate::rpytest_runner::PytestRunner {
    crate::rpytest_runner::PytestRunner::forkserver()
}

pub(crate) fn spawn_forkserver_worker(
    queue: Arc<Mutex<VecDeque<(usize, PytestRunRequest)>>>,
    tx: mpsc::Sender<(usize, Result<PytestRunOutcome, PytestRunError>)>,
) {
    thread::spawn(move || {
        let mut controller: Option<ForkserverController> = None;
        loop {
            let Some((index, req)) = queue.lock().unwrap().pop_front() else {
                break;
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_with_reused_controller(&mut controller, req)
            }))
            .unwrap_or(Err(PytestRunError::WorkerPanic));
            let _ = tx.send((index, result));
        }
    });
}

pub(crate) fn run_with_reused_controller(
    controller: &mut Option<ForkserverController>,
    req: PytestRunRequest,
) -> Result<PytestRunOutcome, PytestRunError> {
    let needs_controller = controller.as_ref().is_none_or(|existing| {
        existing.python != req.python || existing.bootstrap != req.bootstrap
    });
    if needs_controller {
        *controller = Some(ForkserverController::start(&req.python, &req.bootstrap)?);
    }
    controller
        .as_mut()
        .expect("controller initialized")
        .run(req)
}

pub(crate) fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
