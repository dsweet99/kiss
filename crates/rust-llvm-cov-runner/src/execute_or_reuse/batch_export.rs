use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::execute_or_reuse::batch_events::BatchCompilerArtifact;
use crate::execute_or_reuse::batch_export_merge::merge_instance_profile;
use crate::execute_or_reuse::batch_export_resolve::{BinaryIdObjectMap, resolve_objects_for_profdata};
use crate::execute_or_reuse::batch_export_tools::ExportTools;
use crate::{RustLineCoverage, RustLlvmCovError};

pub(crate) use crate::execute_or_reuse::batch_export_merge::{
    export_instance_coverage, merge_profiles,
};

pub(crate) type BatchInstanceExportFn = Arc<
    dyn Fn(
            &InstanceExportRequest,
            &Path,
            &[PathBuf],
            &[PathBuf],
        ) -> Result<RustLineCoverage, RustLlvmCovError>
        + Send
        + Sync,
>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceExportRequest {
    pub instance_id: String,
    pub profile_path: PathBuf,
    pub objects: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportCounters {
    pub export_jobs: usize,
    pub max_active_exports: usize,
    pub max_objects_per_export: usize,
}

#[derive(Clone)]
pub struct SubprocessInstanceExporter {
    tools: ExportTools,
    ignore_filename_regex: Option<String>,
    binary_id_map: Option<BinaryIdObjectMap>,
}

impl SubprocessInstanceExporter {
    pub fn new(tools: ExportTools, ignore_filename_regex: Option<String>) -> Self {
        Self {
            tools,
            ignore_filename_regex,
            binary_id_map: None,
        }
    }

    pub fn with_binary_id_map(
        tools: ExportTools,
        ignore_filename_regex: Option<String>,
        binary_id_map: BinaryIdObjectMap,
    ) -> Self {
        Self {
            tools,
            ignore_filename_regex,
            binary_id_map: Some(binary_id_map),
        }
    }

    pub fn with_catalog_map(
        mut self,
        catalog: &[PathBuf],
        jobs: usize,
    ) -> Result<Self, RustLlvmCovError> {
        self.binary_id_map = Some(BinaryIdObjectMap::build_with_jobs(
            &self.tools, catalog, jobs,
        )?);
        Ok(self)
    }

    pub fn export_instance(
        &self,
        request: &InstanceExportRequest,
        source_root: &Path,
        catalog: &[PathBuf],
        seed_objects: &[PathBuf],
    ) -> Result<RustLineCoverage, RustLlvmCovError> {
        if seed_objects.is_empty() && catalog.is_empty() {
            return Ok(RustLineCoverage {
                files: BTreeMap::new(),
            });
        }
        let profile_dir = request
            .profile_path
            .parent()
            .ok_or_else(|| RustLlvmCovError::InvalidRequest("profile path has no parent".into()))?;
        let profdata_path = profile_dir.join(format!("{}.profdata", request.instance_id));
        if !merge_instance_profile(&self.tools, &request.profile_path, &profdata_path)? {

            return Ok(RustLineCoverage {
                files: BTreeMap::new(),
            });
        }
        let objects = resolve_objects_for_profdata(
            &self.tools,
            &profdata_path,
            catalog,
            seed_objects,
            self.binary_id_map.as_ref(),
        )?;
        export_instance_coverage(
            &self.tools,
            &profdata_path,
            source_root,
            &objects,
            self.ignore_filename_regex.as_deref(),
        )
    }
}

pub fn object_paths_for_executable(
    artifacts: &[BatchCompilerArtifact],
    executable: &Path,
) -> Vec<PathBuf> {
    let mut objects = Vec::new();
    for artifact in artifacts {
        if artifact
            .executable
            .as_ref()
            .is_some_and(|path| paths_equivalent(path, executable))
        {
            objects.extend(crate::execute_or_reuse::batch_export_catalog::object_paths_for_artifact(
                artifact,
            ));
        }
    }
    objects.sort();
    objects.dedup();
    objects
}

fn paths_equivalent(left: &str, right: &Path) -> bool {
    Path::new(left) == right
        || left.ends_with(&format!(
            "/{}",
            right.file_name().unwrap_or_default().to_string_lossy()
        ))
        || right.ends_with(left)
}

pub(crate) fn export_instances_bounded(
    jobs: usize,
    exporter: SubprocessInstanceExporter,
    source_root: &Path,
    catalog: &[PathBuf],
    requests: Vec<InstanceExportRequest>,
) -> Result<(Vec<(String, RustLineCoverage)>, ExportCounters), RustLlvmCovError> {
    let exporter = Arc::new(exporter);
    export_instances_bounded_with(
        jobs,
        source_root,
        catalog,
        requests,
        Arc::new(move |request, root, catalog, seed_objects| {
            exporter.export_instance(request, root, catalog, seed_objects)
        }),
    )
}

pub(crate) fn export_instances_bounded_with(
    jobs: usize,
    source_root: &Path,
    catalog: &[PathBuf],
    requests: Vec<InstanceExportRequest>,
    export_fn: BatchInstanceExportFn,
) -> Result<(Vec<(String, RustLineCoverage)>, ExportCounters), RustLlvmCovError> {
    assert!(jobs > 0, "jobs must be greater than zero");
    if requests.is_empty() {
        return Ok((Vec::new(), ExportCounters::default()));
    }
    let export_fn = export_fn;
    let source_root = source_root.to_path_buf();
    let (tx, rx) = mpsc::channel();
    let active = Arc::new(Mutex::new(0usize));
    let max_objects = requests
        .iter()
        .map(|request| request.objects.len())
        .max()
        .unwrap_or(0);
    let context = _ExportJobContext {
        export_fn,
        source_root,
        catalog: catalog.to_vec(),
        active: active.clone(),
        tx: tx.clone(),
        max_objects,
    };
    let mut running = 0usize;
    let mut next_index = 0usize;
    while next_index < jobs.min(requests.len()) {
        spawn_export_job(
            next_index,
            &requests[next_index],
            _ExportJobContext {
                export_fn: context.export_fn.clone(),
                source_root: context.source_root.clone(),
                catalog: context.catalog.clone(),
                active: context.active.clone(),
                tx: context.tx.clone(),
                max_objects: context.max_objects,
            },
        );
        running += 1;
        next_index += 1;
    }
    let mut results = vec![
        (
            String::new(),
            RustLineCoverage {
                files: BTreeMap::new(),
            }
        );
        requests.len()
    ];
    let mut counters = ExportCounters {
        export_jobs: requests.len(),
        max_active_exports: running,
        max_objects_per_export: max_objects,
    };
    let mut drain = ExportDrainState {
        rx: &rx,
        running: &mut running,
        next_index: &mut next_index,
        requests: &requests,
        active: &active,
        results: &mut results,
        counters: &mut counters,
        context: &context,
    };
    drain_export_results(&mut drain)?;
    Ok((results, counters))
}

struct ExportDrainState<'a> {
    rx: &'a mpsc::Receiver<(usize, ExportJobResult)>,
    running: &'a mut usize,
    next_index: &'a mut usize,
    requests: &'a [InstanceExportRequest],
    active: &'a Arc<Mutex<usize>>,
    results: &'a mut [(String, RustLineCoverage)],
    counters: &'a mut ExportCounters,
    context: &'a _ExportJobContext,
}

struct _ExportJobContext {
    export_fn: BatchInstanceExportFn,
    source_root: PathBuf,
    catalog: Vec<PathBuf>,
    active: Arc<Mutex<usize>>,
    tx: mpsc::Sender<(usize, ExportJobResult)>,
    max_objects: usize,
}

fn spawn_export_job(index: usize, request: &InstanceExportRequest, context: _ExportJobContext) {
    let request = request.clone();
    thread::spawn(move || {
        {
            let mut guard = context.active.lock().expect("active lock");
            *guard += 1;
        }
        let selected = request.objects.as_slice();
        let outcome =
            (context.export_fn)(&request, &context.source_root, &context.catalog, selected)
                .map(|coverage| (request.instance_id.clone(), coverage))
                .map_err(|err| {
                    if selected.len() > context.max_objects {
                        RustLlvmCovError::InvalidRequest(format!(
                            "export for {} passed {} objects; expected at most {}",
                            request.instance_id,
                            selected.len(),
                            context.max_objects
                        ))
                    } else {
                        err
                    }
                });
        {
            let mut guard = context.active.lock().expect("active lock");
            *guard = guard.saturating_sub(1);
        }
        let _ = context.tx.send((index, outcome));
    });
}

fn clone_export_context(context: &_ExportJobContext) -> _ExportJobContext {
    _ExportJobContext {
        export_fn: context.export_fn.clone(),
        source_root: context.source_root.clone(),
        catalog: context.catalog.clone(),
        active: context.active.clone(),
        tx: context.tx.clone(),
        max_objects: context.max_objects,
    }
}

fn drain_export_results(drain: &mut ExportDrainState<'_>) -> Result<(), RustLlvmCovError> {
    while *drain.running > 0 {
        if crate::execute_or_reuse::batch_process_tree::batch_scope_interrupted() {
            return Err(RustLlvmCovError::Interrupted);
        }
        match drain.rx.recv_timeout(Duration::from_millis(25)) {
            Ok((index, outcome)) => {
                *drain.running -= 1;
                let (id, coverage) = outcome?;
                drain.results[index] = (id, coverage);
                if *drain.next_index < drain.requests.len() {
                    spawn_export_job(
                        *drain.next_index,
                        &drain.requests[*drain.next_index],
                        clone_export_context(drain.context),
                    );
                    *drain.running += 1;
                    *drain.next_index += 1;
                    let active_count = *drain.active.lock().expect("active lock");
                    drain.counters.max_active_exports =
                        drain.counters.max_active_exports.max(active_count);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if crate::execute_or_reuse::batch_process_tree::batch_scope_interrupted() {
        return Err(RustLlvmCovError::Interrupted);
    }
    Ok(())
}

type ExportJobResult = Result<(String, RustLineCoverage), RustLlvmCovError>;

#[cfg(test)]
#[path = "batch_export_test.rs"]
mod tests;

#[cfg(test)]
pub use tests::{FakeInstanceExporter, write_fake_profile};

#[cfg(test)]
#[path = "batch_export_exporter_test.rs"]
mod exporter_tests;
