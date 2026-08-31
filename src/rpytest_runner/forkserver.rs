use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
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
        crate::rpytest_runner::runner::collect_bounded_results(
            reqs,
            max_jobs,
            |reqs, max_jobs, on_complete| {
                self.run_many_bounded_with_on_complete(reqs, max_jobs, on_complete);
            },
        )
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

        let partitions =
            partition_requests_by_module(reqs.into_iter().enumerate().collect(), max_jobs);
        let (tx, rx) = mpsc::channel();
        for partition in partitions {
            spawn_forkserver_worker(Arc::new(Mutex::new(partition.into())), tx.clone());
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
            let Some(first) = queue.lock().unwrap().pop_front() else {
                break;
            };
            let batch = take_same_module_batch(&queue, first);
            send_worker_batch(&mut controller, batch, &tx);
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

fn take_same_module_batch(
    queue: &Mutex<VecDeque<(usize, PytestRunRequest)>>,
    first: (usize, PytestRunRequest),
) -> Vec<(usize, PytestRunRequest)> {
    let key = module_key(&first.1.nodeid).to_string();
    let mut batch = vec![first];
    let mut guard = queue.lock().unwrap();
    while guard
        .front()
        .is_some_and(|(_, req)| module_key(&req.nodeid) == key)
    {
        batch.push(guard.pop_front().expect("same-module front exists"));
    }
    batch
}

fn send_worker_batch(
    controller: &mut Option<ForkserverController>,
    batch: Vec<(usize, PytestRunRequest)>,
    tx: &mpsc::Sender<(usize, Result<PytestRunOutcome, PytestRunError>)>,
) {
    let indexes: Vec<usize> = batch.iter().map(|(index, _)| *index).collect();
    let reqs: Vec<PytestRunRequest> = batch.into_iter().map(|(_, req)| req).collect();
    let outcomes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_module_with_reused_controller(controller, reqs)
    }))
    .unwrap_or_else(|_| {
        indexes
            .iter()
            .map(|_| Err(PytestRunError::WorkerPanic))
            .collect()
    });
    for (index, result) in indexes.into_iter().zip(outcomes) {
        let _ = tx.send((index, result));
    }
}

fn run_module_with_reused_controller(
    controller: &mut Option<ForkserverController>,
    reqs: Vec<PytestRunRequest>,
) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
    if reqs.len() == 1 {
        let req = reqs.into_iter().next().expect("one request");
        return vec![run_with_reused_controller(controller, req)];
    }
    let first = reqs.first().expect("module batch is non-empty");
    let needs_controller = controller.as_ref().is_none_or(|existing| {
        existing.python != first.python || existing.bootstrap != first.bootstrap
    });
    if needs_controller {
        match ForkserverController::start(&first.python, &first.bootstrap) {
            Ok(started) => *controller = Some(started),
            Err(err) => {
                return reqs.iter().map(|_| Err(err.cloned())).collect();
            }
        }
    }
    controller
        .as_mut()
        .expect("controller initialized")
        .run_module(reqs)
}

pub(crate) fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn module_key(nodeid: &str) -> &str {
    nodeid.split_once("::").map_or(nodeid, |(module, _)| module)
}

fn partition_requests_by_module(
    reqs: Vec<(usize, PytestRunRequest)>,
    jobs: usize,
) -> Vec<Vec<(usize, PytestRunRequest)>> {
    let mut groups: std::collections::BTreeMap<String, Vec<(usize, PytestRunRequest)>> =
        std::collections::BTreeMap::new();
    let mut order = Vec::new();
    for item in reqs {
        let key = module_key(&item.1.nodeid).to_string();
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(item);
    }
    let grouped: Vec<Vec<_>> = order
        .into_iter()
        .filter_map(|key| groups.remove(&key))
        .collect();
    assign_module_groups(grouped, jobs)
}

fn assign_module_groups(
    mut groups: Vec<Vec<(usize, PytestRunRequest)>>,
    jobs: usize,
) -> Vec<Vec<(usize, PytestRunRequest)>> {
    groups.retain(|group| !group.is_empty());
    if groups.is_empty() {
        return Vec::new();
    }
    let worker_count = jobs.max(1);
    if groups.len() < worker_count {
        return round_robin_requests(groups.into_iter().flatten().collect(), worker_count);
    }
    groups.sort_by_key(|group| std::cmp::Reverse(group.len()));
    let mut workers: Vec<(usize, Vec<(usize, PytestRunRequest)>)> =
        (0..worker_count).map(|_| (0, Vec::new())).collect();
    for group in groups {
        let added = group.len();
        let index = workers
            .iter()
            .enumerate()
            .min_by_key(|(_, (count, _))| *count)
            .map(|(index, _)| index)
            .unwrap_or(0);
        workers[index].0 += added;
        workers[index].1.extend(group);
    }
    workers.into_iter().map(|(_, reqs)| reqs).collect()
}

fn round_robin_requests(
    reqs: Vec<(usize, PytestRunRequest)>,
    jobs: usize,
) -> Vec<Vec<(usize, PytestRunRequest)>> {
    let mut workers = vec![Vec::new(); jobs];
    for (offset, item) in reqs.into_iter().enumerate() {
        workers[offset % jobs].push(item);
    }
    workers
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
mod partition_tests {
    use super::{module_key, partition_requests_by_module};
    use crate::rpytest_runner::PytestRunRequest;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn req(nodeid: &str) -> PytestRunRequest {
        PytestRunRequest::from_parts(
            nodeid.to_string(),
            PathBuf::from("."),
            PathBuf::from("python"),
            Vec::new(),
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
    }

    #[test]
    fn module_key_uses_path_before_separator() {
        assert_eq!(module_key("a.py::test_x"), "a.py");
        assert_eq!(module_key("no_sep"), "no_sep");
    }

    #[test]
    fn many_files_keep_same_module_on_one_worker() {
        let reqs = vec![
            (0, req("a.py::t1")),
            (1, req("a.py::t2")),
            (2, req("b.py::t1")),
            (3, req("b.py::t2")),
        ];
        let parts = partition_requests_by_module(reqs, 2);
        assert_eq!(parts.len(), 2);
        for part in parts {
            let modules: Vec<_> = part
                .iter()
                .map(|(_, request)| module_key(&request.nodeid))
                .collect();
            assert!(
                modules.iter().all(|module| *module == modules[0]),
                "each worker should own whole files when files >= jobs: {modules:?}"
            );
        }
    }

    #[test]
    fn few_files_still_use_requested_jobs() {
        let reqs = vec![
            (0, req("a.py::t1")),
            (1, req("a.py::t2")),
            (2, req("a.py::t3")),
        ];
        let parts = partition_requests_by_module(reqs, 2);
        assert_eq!(parts.len(), 2);
    }
}
