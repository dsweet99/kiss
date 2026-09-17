use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analyze_cache::fnv1a64;

const SCHEMA: &str = "python-file-nodeids-v1";
const CACHE_FILE: &str = "python_file_nodeids.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct FileNodeidCache {
    schema_version: String,
    entries: BTreeMap<String, FileNodeidEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FileNodeidEntry {
    content_fp: String,
    nodeids: Vec<String>,
}

pub(crate) fn lookup_python_file_nodeids(repo_root: &Path, abs: &Path) -> Option<Vec<String>> {
    let rel = repo_relative(repo_root, abs)?;
    let fp = content_fingerprint(abs)?;
    let cache = read_cache(repo_root)?;
    let entry = cache.entries.get(&rel)?;
    (entry.content_fp == fp).then(|| entry.nodeids.clone())
}

pub(crate) fn store_python_file_nodeids(
    repo_root: &Path,
    updates: &[(PathBuf, Vec<String>)],
) -> bool {
    if updates.is_empty() {
        return true;
    }
    let mut cache = read_cache(repo_root).unwrap_or_else(|| FileNodeidCache {
        schema_version: SCHEMA.to_string(),
        entries: BTreeMap::new(),
    });
    if cache.schema_version != SCHEMA {
        cache.schema_version = SCHEMA.to_string();
        cache.entries.clear();
    }
    for (abs, nodeids) in updates {
        let Some(rel) = repo_relative(repo_root, abs) else {
            continue;
        };
        let Some(fp) = content_fingerprint(abs) else {
            continue;
        };
        cache.entries.insert(
            rel,
            FileNodeidEntry {
                content_fp: fp,
                nodeids: nodeids.clone(),
            },
        );
    }
    write_cache(repo_root, &cache)
}

pub(crate) fn repo_relative(repo_root: &Path, abs: &Path) -> Option<String> {
    let root = repo_root.canonicalize().ok()?;
    let abs = abs.canonicalize().ok()?;
    abs.strip_prefix(&root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

fn content_fingerprint(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let len = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, b"python-file-nodeid-fp-v1");
    h = fnv1a64(h, &len.to_le_bytes());
    h = fnv1a64(h, &mtime.to_le_bytes());
    Some(format!("{h:016x}"))
}

fn cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join(CACHE_FILE)
}

fn read_cache(repo_root: &Path) -> Option<FileNodeidCache> {
    let bytes = fs::read(cache_path(repo_root)).ok()?;
    let cache: FileNodeidCache = serde_json::from_slice(&bytes).ok()?;
    (cache.schema_version == SCHEMA).then_some(cache)
}

fn write_cache(repo_root: &Path, cache: &FileNodeidCache) -> bool {
    let dir = repo_root.join(".kiss");
    if fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let Ok(bytes) = serde_json::to_vec_pretty(cache) else {
        return false;
    };
    let tmp = dir.join(format!("{CACHE_FILE}.tmp"));
    if fs::write(&tmp, &bytes).is_err() {
        return false;
    }
    fs::rename(&tmp, cache_path(repo_root)).is_ok()
}

#[cfg(test)]
#[path = "python_nodeid_cache_test.rs"]
mod python_nodeid_cache_test;
