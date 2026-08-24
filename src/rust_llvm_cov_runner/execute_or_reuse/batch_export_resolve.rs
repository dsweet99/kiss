use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_tools::{ExportTools, read_profdata_binary_ids};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BinaryIdObjectMap {
    id_to_object: std::collections::BTreeMap<String, PathBuf>,
}

impl BinaryIdObjectMap {
    pub fn build(tools: &ExportTools, catalog: &[PathBuf]) -> Result<Self, RustLlvmCovError> {
        Self::build_with_jobs(tools, catalog, 1)
    }

    pub fn build_with_jobs(
        tools: &ExportTools,
        catalog: &[PathBuf],
        jobs: usize,
    ) -> Result<Self, RustLlvmCovError> {
        assert!(jobs > 0, "jobs must be greater than zero");
        let ids = read_catalog_binary_ids(tools, catalog, jobs)?;
        let mut id_to_object = std::collections::BTreeMap::new();
        for (object, id) in catalog.iter().zip(ids) {
            let Some(id) = id else {
                continue;
            };
            insert_catalog_object(&mut id_to_object, object, id)?;
        }
        Ok(Self { id_to_object })
    }

    pub fn lookup(&self, id: &str) -> Option<&PathBuf> {
        self.id_to_object.get(id)
    }

    pub fn lookup_by_object(&self, object: &Path) -> Option<&str> {
        self.id_to_object
            .iter()
            .find_map(|(id, path)| (path == object).then_some(id.as_str()))
    }
}

fn insert_catalog_object(
    id_to_object: &mut std::collections::BTreeMap<String, PathBuf>,
    object: &Path,
    id: String,
) -> Result<(), RustLlvmCovError> {
    match id_to_object.get(&id) {
        None => {
            id_to_object.insert(id, object.to_path_buf());
        }
        Some(existing) if existing == object => {}
        Some(existing) => {
            let existing_in_deps = existing
                .parent()
                .and_then(|parent| parent.file_name())
                .is_some_and(|name| name == "deps");
            let candidate_in_deps = object
                .parent()
                .and_then(|parent| parent.file_name())
                .is_some_and(|name| name == "deps");
            if existing_in_deps == candidate_in_deps {
                return Err(RustLlvmCovError::InvalidRequest(format!(
                    "ambiguous catalog objects [{existing:?}, {object:?}] for binary id `{id}`"
                )));
            }
            let preferred = if candidate_in_deps {
                object.to_path_buf()
            } else {
                existing.clone()
            };
            id_to_object.insert(id, preferred);
        }
    }
    Ok(())
}

fn read_catalog_binary_ids(
    tools: &ExportTools,
    catalog: &[PathBuf],
    jobs: usize,
) -> Result<Vec<Option<String>>, RustLlvmCovError> {
    if catalog.is_empty() {
        return Ok(Vec::new());
    }
    if jobs == 1 || catalog.len() == 1 {
        return catalog
            .iter()
            .map(|object| read_object_binary_id(tools, object))
            .collect();
    }
    let worker_count = jobs.min(catalog.len());
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let tx = tx.clone();
            let next = &next;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= catalog.len() {
                        break;
                    }
                    let result = read_object_binary_id(tools, &catalog[index]);
                    if tx.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(tx);
    let mut slots = vec![None; catalog.len()];
    let mut received = 0usize;
    while let Ok((index, result)) = rx.recv() {
        slots[index] = Some(result?);
        received += 1;
    }
    if received != catalog.len() {
        return Err(RustLlvmCovError::InvalidRequest(
            "catalog binary-id workers exited before covering every object".into(),
        ));
    }
    Ok(slots.into_iter().map(Option::unwrap).collect())
}

pub(crate) fn resolve_objects_for_profdata(
    tools: &ExportTools,
    profdata: &Path,
    catalog: &[PathBuf],
    seed_objects: &[PathBuf],
    binary_id_map: Option<&BinaryIdObjectMap>,
) -> Result<Vec<PathBuf>, RustLlvmCovError> {
    if seed_objects.is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "export requires seed objects derived from the instance executable".into(),
        ));
    }
    let binary_id_map = binary_id_map.ok_or_else(|| {
        RustLlvmCovError::InvalidRequest(
            "export requires a binary-id object map built from the workspace catalog".into(),
        )
    })?;
    let profile_ids = read_profdata_binary_ids(tools, profdata)?;
    if profile_ids.is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "profile {} has no binary ids; rebuild with GNU build-id instrumentation",
            profdata.display()
        )));
    }
    resolve_objects_by_binary_ids(
        tools,
        profdata,
        &profile_ids,
        catalog,
        seed_objects,
        binary_id_map,
    )
}

fn resolve_objects_by_binary_ids(
    tools: &ExportTools,
    profdata: &Path,
    profile_ids: &[String],
    catalog: &[PathBuf],
    seed_objects: &[PathBuf],
    binary_id_map: &BinaryIdObjectMap,
) -> Result<Vec<PathBuf>, RustLlvmCovError> {
    let mut seed_ids = BTreeSet::new();
    for seed in seed_objects {
        if let Some(id) = binary_id_map.lookup_by_object(seed) {
            seed_ids.insert(id.to_string());
        } else if let Some(id) = read_object_binary_id(tools, seed)? {
            seed_ids.insert(id);
        }
    }
    let mut resolved = Vec::new();
    let mut unmatched = Vec::new();
    for id in profile_ids {
        if !seed_ids.is_empty() && !seed_ids.contains(id) {
            continue;
        }
        match try_resolve_object_for_binary_id(tools, catalog, seed_objects, binary_id_map, id)? {
            Some(path) => resolved.push(path),
            None => unmatched.push(id.clone()),
        }
    }
    resolved.sort();
    resolved.dedup();
    if unmatched.is_empty() {
        if resolved.is_empty() {
            return Err(RustLlvmCovError::InvalidRequest(format!(
                "seed-filtered object resolve produced no objects for {}; \
                 seed build-ids may be absent from the merged profile (stale pools?)",
                profdata.display()
            )));
        }

        return Ok(resolved);
    }

    resolve_with_orphan_profile_ids(
        tools,
        profdata,
        profile_ids,
        seed_objects,
        resolved,
        &unmatched,
    )
}

fn resolve_with_orphan_profile_ids(
    tools: &ExportTools,
    profdata: &Path,
    profile_ids: &[String],
    seed_objects: &[PathBuf],
    mut resolved: Vec<PathBuf>,
    unmatched: &[String],
) -> Result<Vec<PathBuf>, RustLlvmCovError> {
    let profile_id_set: BTreeSet<&str> = profile_ids.iter().map(String::as_str).collect();
    let mut covered_seeds = Vec::new();
    for seed in seed_objects {
        if let Some(id) = read_object_binary_id(tools, seed)?
            && profile_id_set.contains(id.as_str())
        {
            covered_seeds.push(seed.clone());
        }
    }
    if covered_seeds.is_empty() {
        let expected_id = unmatched.first().map(String::as_str).unwrap_or("unknown");
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "no catalog object matched profile binary id `{expected_id}` for {}",
            profdata.display()
        )));
    }
    resolved.extend(covered_seeds);
    resolved.sort();
    resolved.dedup();
    Ok(resolved)
}

fn try_resolve_object_for_binary_id(
    _tools: &ExportTools,
    _catalog: &[PathBuf],
    _seed_objects: &[PathBuf],
    binary_id_map: &BinaryIdObjectMap,
    expected_id: &str,
) -> Result<Option<PathBuf>, RustLlvmCovError> {
    Ok(binary_id_map.lookup(expected_id).cloned())
}

pub(crate) fn read_object_binary_id(
    tools: &ExportTools,
    object: &Path,
) -> Result<Option<String>, RustLlvmCovError> {
    let output = Command::new(&tools.llvm_readobj)
        .arg("--notes")
        .arg(object)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(RustLlvmCovError::Io)?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_tools::parse_readobj_build_id(&output.stdout))
}

#[cfg(test)]
#[path = "batch_export_resolve_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_export_resolve_orphan_test.rs"]
mod orphan_tests;
