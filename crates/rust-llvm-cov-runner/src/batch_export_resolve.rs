use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::RustLlvmCovError;
use crate::batch_export_tools::{ExportTools, objects_satisfy_profile, read_profdata_binary_ids};

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
    let mut resolved = Vec::new();
    for id in profile_ids {
        resolved.push(resolve_object_for_binary_id(
            tools,
            profdata,
            catalog,
            seed_objects,
            binary_id_map,
            id,
        )?);
    }
    resolved.sort();
    resolved.dedup();
    if !objects_satisfy_profile(tools, profdata, &resolved) {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "binary-id-resolved objects {:?} do not satisfy profile {}",
            resolved,
            profdata.display()
        )));
    }
    Ok(resolved)
}

fn resolve_object_for_binary_id(
    tools: &ExportTools,
    profdata: &Path,
    catalog: &[PathBuf],
    seed_objects: &[PathBuf],
    binary_id_map: &BinaryIdObjectMap,
    expected_id: &str,
) -> Result<PathBuf, RustLlvmCovError> {
    if let Some(path) = binary_id_map.lookup(expected_id) {
        return Ok(path.clone());
    }
    let mut search_paths = catalog
        .iter()
        .chain(seed_objects.iter())
        .collect::<Vec<_>>();
    search_paths.sort();
    search_paths.dedup();
    let mut matches = Vec::new();
    for object in search_paths {
        if read_object_binary_id(tools, object)?.as_deref() == Some(expected_id) {
            matches.push(object.clone());
        }
    }
    match matches.len() {
        0 => Err(RustLlvmCovError::InvalidRequest(format!(
            "no catalog object matched profile binary id `{expected_id}` for {}",
            profdata.display()
        ))),
        1 => Ok(matches[0].clone()),
        _ => Err(RustLlvmCovError::InvalidRequest(format!(
            "ambiguous catalog objects {:?} for profile binary id `{expected_id}`",
            matches
        ))),
    }
}

fn read_object_binary_id(
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
