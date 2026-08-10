//! Cached universe file lists for warm `kiss cov` gather.
//!
//! Invalidation keys on ignore/lang filter plus lightweight coverage-backend
//! identities (same population fingerprints `cov_records_cache` already uses),
//! so a warm hit skips walking the tree.

use std::fs;
use std::path::{Path, PathBuf};

use kiss::Language;
use serde::{Deserialize, Serialize};

use crate::analyze::cov_records_cache::{python_backend_identity_for_file_list, rust_backend_identity_for_file_list};
use crate::analyze_cache::fnv1a64;

const SCHEMA_VERSION: &str = "kiss-cov-file-list-v2";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CovFileListCache {
    schema_version: String,
    fingerprint: String,
    py_files: Vec<PathBuf>,
    rs_files: Vec<PathBuf>,
}

pub(crate) struct CovFileListKey<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) lang_filter: Option<Language>,
    pub(crate) ignore: &'a [String],
}

pub(crate) fn try_load_cov_file_list(key: &CovFileListKey<'_>) -> Option<(Vec<PathBuf>, Vec<PathBuf>)> {
    let fingerprint = file_list_fingerprint(key)?;
    let raw = fs::read(cache_path(key.repo_root)).ok()?;
    let cache: CovFileListCache = serde_json::from_slice(&raw).ok()?;
    if cache.schema_version != SCHEMA_VERSION || cache.fingerprint != fingerprint {
        return None;
    }
    if cache.py_files.is_empty() && cache.rs_files.is_empty() {
        return None;
    }
    Some((cache.py_files, cache.rs_files))
}

pub(crate) fn store_cov_file_list(
    key: &CovFileListKey<'_>,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) {
    let Some(fingerprint) = file_list_fingerprint(key) else {
        return;
    };
    let cache = CovFileListCache {
        schema_version: SCHEMA_VERSION.to_string(),
        fingerprint,
        py_files: py_files.to_vec(),
        rs_files: rs_files.to_vec(),
    };
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        return;
    };
    let path = cache_path(key.repo_root);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, bytes).is_ok() {
        let _ = fs::rename(tmp, path);
    }
}

fn cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("cov_file_list_cache.json")
}

fn file_list_fingerprint(key: &CovFileListKey<'_>) -> Option<String> {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, SCHEMA_VERSION.as_bytes());
    if let Some(lang) = key.lang_filter {
        h = fnv1a64(
            h,
            match lang {
                Language::Python => b"python",
                Language::Rust => b"rust",
            },
        );
    }
    for prefix in key.ignore {
        h = fnv1a64(h, prefix.as_bytes());
        h = fnv1a64(h, &[0]);
    }
    let want_py = matches!(key.lang_filter, None | Some(Language::Python));
    let want_rs = matches!(key.lang_filter, None | Some(Language::Rust));
    if want_py {
        h = fnv1a64(h, python_backend_identity_for_file_list(key.repo_root)?.as_bytes());
    }
    if want_rs {
        h = fnv1a64(h, rust_backend_identity_for_file_list(key.repo_root)?.as_bytes());
    }
    Some(format!("{h:016x}"))
}

#[cfg(test)]
#[path = "cov_file_list_cache_test.rs"]
mod tests;
