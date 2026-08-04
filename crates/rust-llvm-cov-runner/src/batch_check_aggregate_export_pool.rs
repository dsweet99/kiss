use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::batch_export_resolve::{BinaryIdObjectMap, read_object_binary_id};
use crate::batch_export_tools::{ExportTools, read_profdata_binary_ids};
use crate::RustLlvmCovError;

pub(super) fn seed_binary_ids_for_objects(
    tools: &ExportTools,
    binary_id_map: &BinaryIdObjectMap,
    objects: &[PathBuf],
) -> Result<BTreeSet<String>, RustLlvmCovError> {
    let mut ids = BTreeSet::new();
    for object in objects {
        if let Some(id) = binary_id_map.lookup_by_object(object) {
            ids.insert(id.to_string());
            continue;
        }
        if let Some(id) = read_object_binary_id(tools, object)? {
            ids.insert(id);
        }
    }
    Ok(ids)
}

pub(super) fn filter_pool_inputs_for_seed_ids(
    tools: &ExportTools,
    profile_inputs: &[PathBuf],
    seed_ids: &BTreeSet<String>,
    cache: &Arc<Mutex<BTreeMap<PathBuf, Vec<String>>>>,
) -> Result<Vec<PathBuf>, RustLlvmCovError> {
    if seed_ids.is_empty()
        || !profile_inputs
            .iter()
            .any(|path| path.extension().and_then(|ext| ext.to_str()) == Some("profraw"))
    {
        return Ok(profile_inputs.to_vec());
    }
    let mut matched = Vec::new();
    for path in profile_inputs {
        let ids = {
            let mut guard = cache.lock().expect("profraw binary id cache");
            if let Some(ids) = guard.get(path) {
                ids.clone()
            } else {
                let ids = read_profdata_binary_ids(tools, path)?;
                guard.insert(path.clone(), ids.clone());
                ids
            }
        };
        if ids.iter().any(|id| seed_ids.contains(id)) {
            matched.push(path.clone());
        }
    }
    if matched.is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "no pool profraw files matched seed binary id(s): {}",
            seed_ids.iter().cloned().collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(matched)
}

pub(super) fn stable_name(value: &str) -> String {
    let h = crate::rust_cov_cache::rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, value.as_bytes());
    format!("{h:016x}")
}

pub(super) fn resolve_profile_merge_inputs(paths: &[PathBuf]) -> Result<Vec<PathBuf>, RustLlvmCovError> {
    if !paths
        .iter()
        .any(|path| path.to_string_lossy().contains('%'))
    {
        return Ok(paths.to_vec());
    }
    let pattern = paths.first().ok_or_else(|| {
        RustLlvmCovError::InvalidRequest("profile path list is empty".into())
    })?;
    let dir = pattern.parent().ok_or_else(|| {
        RustLlvmCovError::InvalidRequest("profile path has no parent".into())
    })?;
    let prefix = pool_pattern_file_prefix(pattern).ok_or_else(|| {
        RustLlvmCovError::InvalidRequest(format!(
            "cannot derive pool file prefix from {}",
            pattern.display()
        ))
    })?;
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).map_err(RustLlvmCovError::Io)? {
        let path = entry.map_err(RustLlvmCovError::Io)?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(&prefix) && name.ends_with(".profraw") {
            found.push(path);
        }
    }
    if found.is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "coverage profile pool produced no `{prefix}*.profraw` files under {}",
            dir.display()
        )));
    }
    found.sort();
    Ok(found)
}

fn pool_pattern_file_prefix(pattern: &Path) -> Option<String> {
    let name = pattern.file_name()?.to_str()?;
    let before_pct = name.split('%').next()?;
    if before_pct.is_empty() {
        return None;
    }
    Some(before_pct.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn pool_pattern_file_prefix_keeps_text_before_percent() {
        assert_eq!(
            pool_pattern_file_prefix(Path::new("/tmp/pool-%32m.profraw")).as_deref(),
            Some("pool-")
        );
        assert_eq!(pool_pattern_file_prefix(Path::new("%m.profraw")), None);
        assert_eq!(
            pool_pattern_file_prefix(Path::new("/tmp/plain.profraw")).as_deref(),
            Some("plain.profraw")
        );
    }

    #[test]
    fn resolve_profile_merge_inputs_passthrough_without_percent() {
        let paths = vec![PathBuf::from("/tmp/a.profraw"), PathBuf::from("/tmp/b.profraw")];
        let resolved = resolve_profile_merge_inputs(&paths).unwrap();
        assert_eq!(resolved, paths);
    }

    #[test]
    fn resolve_profile_merge_inputs_expands_pool_glob() {
        let tmp = tempfile::tempdir().unwrap();
        let pattern = tmp.path().join("pool-%32m.profraw");
        let keep = tmp.path().join("pool-aaaa.profraw");
        let other = tmp.path().join("other-bbbb.profraw");
        std::fs::write(&keep, b"x").unwrap();
        std::fs::write(&other, b"y").unwrap();
        let found = resolve_profile_merge_inputs(&[pattern]).unwrap();
        assert_eq!(found, vec![keep]);
    }

    #[test]
    fn filter_pool_inputs_passthrough_when_seed_ids_empty() {
        let tools = ExportTools {
            llvm_profdata: PathBuf::from("llvm-profdata"),
            llvm_cov: PathBuf::from("llvm-cov"),
            llvm_readobj: PathBuf::from("llvm-readobj"),
        };
        let inputs = vec![PathBuf::from("/tmp/a.profraw")];
        let cache = Arc::new(Mutex::new(BTreeMap::new()));
        let filtered =
            filter_pool_inputs_for_seed_ids(&tools, &inputs, &BTreeSet::new(), &cache).unwrap();
        assert_eq!(filtered, inputs);
    }

    #[test]
    fn filter_pool_inputs_passthrough_when_no_profraw_extension() {
        let tools = ExportTools {
            llvm_profdata: PathBuf::from("llvm-profdata"),
            llvm_cov: PathBuf::from("llvm-cov"),
            llvm_readobj: PathBuf::from("llvm-readobj"),
        };
        let inputs = vec![PathBuf::from("/tmp/a.profdata")];
        let mut seeds = BTreeSet::new();
        seeds.insert("deadbeef".to_string());
        let cache = Arc::new(Mutex::new(BTreeMap::new()));
        let filtered =
            filter_pool_inputs_for_seed_ids(&tools, &inputs, &seeds, &cache).unwrap();
        assert_eq!(filtered, inputs);
    }

    #[test]
    fn stable_name_is_deterministic_hex() {
        assert_eq!(stable_name("demo"), stable_name("demo"));
        assert_ne!(stable_name("demo"), stable_name("other"));
        assert_eq!(stable_name("demo").len(), 16);
    }

    #[test]
    fn filter_pool_inputs_errors_when_cached_ids_do_not_match_seeds() {
        let tools = ExportTools {
            llvm_profdata: PathBuf::from("llvm-profdata"),
            llvm_cov: PathBuf::from("llvm-cov"),
            llvm_readobj: PathBuf::from("llvm-readobj"),
        };
        let path = PathBuf::from("/tmp/pool-aaaa.profraw");
        let mut seeds = BTreeSet::new();
        seeds.insert("wanted-id".to_string());
        let cache = Arc::new(Mutex::new(BTreeMap::from([(
            path.clone(),
            vec!["other-id".to_string()],
        )])));
        let err = filter_pool_inputs_for_seed_ids(&tools, &[path], &seeds, &cache).unwrap_err();
        match err {
            RustLlvmCovError::InvalidRequest(msg) => {
                assert!(
                    msg.contains("no pool profraw files matched seed binary id"),
                    "{msg}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn resolve_profile_merge_inputs_errors_when_pool_has_no_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let pattern = tmp.path().join("pool-%32m.profraw");
        std::fs::write(tmp.path().join("other-bbbb.profraw"), b"y").unwrap();
        let err = resolve_profile_merge_inputs(&[pattern]).unwrap_err();
        match err {
            RustLlvmCovError::InvalidRequest(msg) => {
                assert!(msg.contains("coverage profile pool produced no"), "{msg}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}

