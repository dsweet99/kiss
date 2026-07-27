use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::RustLlvmCovError;
use crate::batch_export_tools::{ExportTools, read_profdata_binary_ids};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BinaryIdObjectMap {
    id_to_object: std::collections::BTreeMap<String, PathBuf>,
}

impl BinaryIdObjectMap {
    pub fn build(tools: &ExportTools, catalog: &[PathBuf]) -> Result<Self, RustLlvmCovError> {
        let mut id_to_object = std::collections::BTreeMap::new();
        for object in catalog {
            let Some(id) = read_object_binary_id(tools, object)? else {
                continue;
            };
            match id_to_object.get(&id) {
                None => {
                    id_to_object.insert(id, object.clone());
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
                        object.clone()
                    } else {
                        existing.clone()
                    };
                    id_to_object.insert(id, preferred);
                }
            }
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
            // Shared LLVM profile pools contain every binary id. Keep only the
            // seed binary's objects so aggregate line maps stay per-binary.
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
        // Binary-id resolution already matched every selected profile id to a catalog object.
        // Skip the redundant full `llvm-cov export -check-binary-ids` validation export.
        return Ok(resolved);
    }
    // Nested coverage / deleted artifacts can leave orphan binary ids in a
    // merged profile. Keep seed objects whose build-ids are still present.
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
    // The catalog map is authoritative. Rescanning every object with
    // llvm-readobj on each miss is O(orphans × catalog) and pathologically
    // slow for large binaries; orphan seeds are handled separately.
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
    Ok(crate::batch_export_tools::parse_readobj_build_id(
        &output.stdout,
    ))
}

#[cfg(test)]
#[path = "batch_export_resolve_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_export_resolve_orphan_test.rs"]
mod orphan_tests;
