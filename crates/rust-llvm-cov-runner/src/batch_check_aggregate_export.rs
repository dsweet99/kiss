use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::batch_aggregate::InstanceResult;
use crate::batch_events::BatchCompilerArtifact;
use crate::batch_export::{
    ExportCounters, export_instance_coverage, merge_profiles, object_paths_for_executable,
};
use crate::batch_export_resolve::{BinaryIdObjectMap, resolve_objects_for_profdata};
use crate::batch_export_tools::{ExportTools, resolve_export_tools_from_rustc};
use crate::batch_shim::BatchShimMetadata;
use crate::batch_shim_lookup::resolve_shim_metadata;
use crate::{RustLineCoverage, RustLlvmCovError};

#[path = "batch_check_aggregate_export_pool.rs"]
mod batch_check_aggregate_export_pool;
use batch_check_aggregate_export_pool::{
    filter_pool_inputs_for_seed_ids, resolve_profile_merge_inputs, seed_binary_ids_for_objects,
    stable_name,
};

type CheckAggregateExportFn = Arc<
    dyn Fn(
            &CheckAggregateExportRequest,
            &Path,
            &[PathBuf],
        ) -> Result<(String, RustLineCoverage), RustLlvmCovError>
        + Send
        + Sync,
>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckAggregateExportRequest {
    pub binary_id: String,
    pub instance_names: Vec<String>,
    pub profile_paths: Vec<PathBuf>,
    pub objects: Vec<PathBuf>,
}

pub(crate) fn build_check_aggregate_export_requests(
    instances: &[InstanceResult],
    shim_metadata: &[BatchShimMetadata],
    artifacts: &[BatchCompilerArtifact],
    publication_binary_ids: Option<&BTreeSet<String>>,
) -> Result<Vec<CheckAggregateExportRequest>, RustLlvmCovError> {
    let metadata_by_id: BTreeMap<_, _> = shim_metadata
        .iter()
        .map(|item| (item.full_name.clone(), item))
        .collect();
    let mut groups = BTreeMap::<String, Vec<&InstanceResult>>::new();
    for instance in instances.iter().filter(|instance| instance.passed) {
        if publication_binary_ids.is_some_and(|ids| !ids.contains(&instance.test_binary_id)) {
            continue;
        }
        groups
            .entry(instance.test_binary_id.clone())
            .or_default()
            .push(instance);
    }
    let requests = export_requests_from_groups(groups, &metadata_by_id, shim_metadata, artifacts)?;
    reject_missing_publication_binaries(publication_binary_ids, &requests)?;
    Ok(requests)
}

fn export_requests_from_groups(
    groups: BTreeMap<String, Vec<&InstanceResult>>,
    metadata_by_id: &BTreeMap<String, &BatchShimMetadata>,
    shim_metadata: &[BatchShimMetadata],
    artifacts: &[BatchCompilerArtifact],
) -> Result<Vec<CheckAggregateExportRequest>, RustLlvmCovError> {
    let mut requests = Vec::new();
    for (binary_id, mut group) in groups {
        group.sort_by(|left, right| left.full_name.cmp(&right.full_name));
        requests.push(export_request_from_group(
            binary_id,
            group,
            metadata_by_id,
            shim_metadata,
            artifacts,
        )?);
    }
    Ok(requests)
}

fn export_request_from_group(
    binary_id: String,
    group: Vec<&InstanceResult>,
    metadata_by_id: &BTreeMap<String, &BatchShimMetadata>,
    shim_metadata: &[BatchShimMetadata],
    artifacts: &[BatchCompilerArtifact],
) -> Result<CheckAggregateExportRequest, RustLlvmCovError> {
    let mut instance_names = Vec::new();
    let mut profile_paths = Vec::new();
    let mut objects = Vec::new();
    for instance in group {
        let shim = resolve_shim_metadata(metadata_by_id, shim_metadata, &instance.full_name)?;
        let executable = shim.argv.first().ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "missing test binary argv for export instance `{}`",
                instance.full_name
            ))
        })?;
        instance_names.push(instance.full_name.clone());
        profile_paths.push(shim.profile_path.clone());
        objects.extend(object_paths_for_executable(
            artifacts,
            Path::new(executable),
        ));
    }
    profile_paths.sort();
    profile_paths.dedup();
    objects.sort();
    objects.dedup();
    if profile_paths.is_empty() || objects.is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "aggregate export for binary `{binary_id}` has no profiles or objects"
        )));
    }
    Ok(CheckAggregateExportRequest {
        binary_id,
        instance_names,
        profile_paths,
        objects,
    })
}

fn reject_missing_publication_binaries(
    publication_binary_ids: Option<&BTreeSet<String>>,
    requests: &[CheckAggregateExportRequest],
) -> Result<(), RustLlvmCovError> {
    let Some(publication_binary_ids) = publication_binary_ids else {
        return Ok(());
    };
    let observed: BTreeSet<_> = requests.iter().map(|req| req.binary_id.clone()).collect();
    let missing: Vec<_> = publication_binary_ids
        .difference(&observed)
        .cloned()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(RustLlvmCovError::InvalidRequest(format!(
        "aggregate repair did not produce successful profiles for binary id(s): {}",
        missing.join(", ")
    )))
}

pub(crate) fn export_check_aggregates_bounded(
    jobs: usize,
    source_root: &Path,
    catalog: &[PathBuf],
    requests: Vec<CheckAggregateExportRequest>,
) -> Result<(BTreeMap<String, RustLineCoverage>, ExportCounters), RustLlvmCovError> {
    assert!(jobs > 0, "jobs must be greater than zero");
    if requests.is_empty() {
        return Ok((BTreeMap::new(), ExportCounters::default()));
    }
    let tools = resolve_export_tools_from_rustc(std::ffi::OsStr::new("rustc"))?;
    let binary_id_map = BinaryIdObjectMap::build(&tools, catalog)?;
    let exporter = CheckAggregateExporter {
        tools,
        binary_id_map,
        profraw_binary_ids: Arc::new(Mutex::new(BTreeMap::new())),
    };
    let exporter: CheckAggregateExportFn = Arc::new(move |request, source_root, catalog| {
        exporter.export_binary(request, source_root, catalog)
    });
    export_check_aggregates_bounded_with(jobs, source_root, catalog, requests, exporter)
}

fn export_check_aggregates_bounded_with(
    jobs: usize,
    source_root: &Path,
    catalog: &[PathBuf],
    requests: Vec<CheckAggregateExportRequest>,
    exporter: CheckAggregateExportFn,
) -> Result<(BTreeMap<String, RustLineCoverage>, ExportCounters), RustLlvmCovError> {
    assert!(jobs > 0, "jobs must be greater than zero");
    if requests.is_empty() {
        return Ok((BTreeMap::new(), ExportCounters::default()));
    }
    let mut scheduler = ExportScheduler::new(jobs, source_root, catalog, requests, exporter);
    scheduler.run()
}

struct ExportScheduler {
    source_root: PathBuf,
    catalog: Vec<PathBuf>,
    requests: Vec<CheckAggregateExportRequest>,
    exporter: CheckAggregateExportFn,
    active: Arc<Mutex<usize>>,
    tx: mpsc::Sender<(usize, CheckAggregateExportResult)>,
    rx: mpsc::Receiver<(usize, CheckAggregateExportResult)>,
    running: usize,
    next_index: usize,
    counters: ExportCounters,
}

impl ExportScheduler {
    fn new(
        jobs: usize,
        source_root: &Path,
        catalog: &[PathBuf],
        requests: Vec<CheckAggregateExportRequest>,
        exporter: CheckAggregateExportFn,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let counters = ExportCounters {
            export_jobs: requests.len(),
            max_active_exports: jobs.min(requests.len()),
            max_objects_per_export: requests
                .iter()
                .map(|request| request.objects.len())
                .max()
                .unwrap_or(0),
        };
        Self {
            source_root: source_root.to_path_buf(),
            catalog: catalog.to_vec(),
            requests,
            exporter,
            active: Arc::new(Mutex::new(0)),
            tx,
            rx,
            running: 0,
            next_index: 0,
            counters,
        }
    }

    fn run(
        &mut self,
    ) -> Result<(BTreeMap<String, RustLineCoverage>, ExportCounters), RustLlvmCovError> {
        self.spawn_initial_exports();
        let mut results = BTreeMap::new();
        while self.running > 0 {
            if crate::batch_process_tree::batch_scope_interrupted() {
                return Err(RustLlvmCovError::InvalidRequest("batch interrupted".into()));
            }
            self.receive_one(&mut results)?;
        }
        if crate::batch_process_tree::batch_scope_interrupted() {
            return Err(RustLlvmCovError::InvalidRequest("batch interrupted".into()));
        }
        Ok((results, self.counters.clone()))
    }

    fn spawn_initial_exports(&mut self) {
        let limit = self.counters.max_active_exports;
        while self.next_index < limit {
            self.spawn_next_export();
        }
    }

    fn receive_one(
        &mut self,
        results: &mut BTreeMap<String, RustLineCoverage>,
    ) -> Result<(), RustLlvmCovError> {
        match self.rx.recv_timeout(Duration::from_millis(25)) {
            Ok((_, outcome)) => {
                self.running -= 1;
                let (binary_id, coverage) = outcome?;
                results.insert(binary_id, coverage);
                self.spawn_next_export();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => self.running = 0,
        }
        Ok(())
    }

    fn spawn_next_export(&mut self) {
        if self.next_index >= self.requests.len() {
            return;
        }
        spawn_check_aggregate_export(
            self.next_index,
            &self.requests[self.next_index],
            CheckAggregateExportContext {
                exporter: self.exporter.clone(),
                source_root: self.source_root.clone(),
                catalog: self.catalog.clone(),
                active: self.active.clone(),
                tx: self.tx.clone(),
            },
        );
        self.running += 1;
        self.next_index += 1;
        let active_count = *self.active.lock().expect("active lock");
        self.counters.max_active_exports = self.counters.max_active_exports.max(active_count);
    }
}

struct CheckAggregateExportContext {
    exporter: CheckAggregateExportFn,
    source_root: PathBuf,
    catalog: Vec<PathBuf>,
    active: Arc<Mutex<usize>>,
    tx: mpsc::Sender<(usize, CheckAggregateExportResult)>,
}

type CheckAggregateExportResult = Result<(String, RustLineCoverage), RustLlvmCovError>;

fn spawn_check_aggregate_export(
    index: usize,
    request: &CheckAggregateExportRequest,
    context: CheckAggregateExportContext,
) {
    let request = request.clone();
    std::thread::spawn(move || {
        {
            let mut guard = context.active.lock().expect("active lock");
            *guard += 1;
        }
        let outcome = (context.exporter)(&request, &context.source_root, &context.catalog);
        {
            let mut guard = context.active.lock().expect("active lock");
            *guard = guard.saturating_sub(1);
        }
        let _ = context.tx.send((index, outcome));
    });
}

#[derive(Clone)]
struct CheckAggregateExporter {
    tools: ExportTools,
    binary_id_map: BinaryIdObjectMap,
    /// Cache of pool profraw → binary ids (shared across export workers).
    profraw_binary_ids: Arc<Mutex<BTreeMap<PathBuf, Vec<String>>>>,
}

impl CheckAggregateExporter {
    fn export_binary(
        &self,
        request: &CheckAggregateExportRequest,
        source_root: &Path,
        catalog: &[PathBuf],
    ) -> Result<(String, RustLineCoverage), RustLlvmCovError> {
        let profile_dir = request
            .profile_paths
            .first()
            .and_then(|path| path.parent())
            .ok_or_else(|| RustLlvmCovError::InvalidRequest("profile path has no parent".into()))?;
        let profdata = profile_dir.join(format!(
            "check-aggregate-{}.profdata",
            stable_name(&request.binary_id)
        ));
        let profile_inputs = resolve_profile_merge_inputs(&request.profile_paths)?;
        let seed_ids = seed_binary_ids_for_objects(&self.tools, &self.binary_id_map, &request.objects)?;
        let filtered_inputs = filter_pool_inputs_for_seed_ids(
            &self.tools,
            &profile_inputs,
            &seed_ids,
            &self.profraw_binary_ids,
        )?;
        merge_profiles(&self.tools, &filtered_inputs, &profdata)?;
        let objects = resolve_objects_for_profdata(
            &self.tools,
            &profdata,
            catalog,
            &request.objects,
            Some(&self.binary_id_map),
        )?;
        let coverage =
            export_instance_coverage(&self.tools, &profdata, source_root, &objects, None)?;
        Ok((request.binary_id.clone(), coverage))
    }
}

#[cfg(test)]
#[path = "batch_check_aggregate_export_test.rs"]
mod tests;
