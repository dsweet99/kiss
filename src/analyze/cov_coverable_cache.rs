use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analyze::line_coverage::{CoverableDenom, CoverageSourceFacts};
use crate::analyze_cache::fnv1a64;

const SCHEMA_VERSION: &str = "kiss-cov-coverable-v2";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CovCoverableCache {
    schema_version: String,
    fingerprint: String,
    denoms: Vec<CoverableDenom>,
}

#[derive(Clone, Debug)]
pub(crate) struct CovCoverableKey<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) py_files: &'a [PathBuf],
    pub(crate) rs_files: &'a [PathBuf],
    pub(crate) ignore: &'a [String],
    pub(crate) lang_filter: Option<&'a str>,
}

pub(crate) fn try_load_coverable_denoms(key: &CovCoverableKey<'_>) -> Option<Vec<CoverableDenom>> {
    let fingerprint = coverable_fingerprint(key)?;
    let raw = fs::read(cache_path(key.repo_root)).ok()?;
    let cache: CovCoverableCache = serde_json::from_slice(&raw).ok()?;
    if cache.schema_version != SCHEMA_VERSION || cache.fingerprint != fingerprint {
        return None;
    }
    Some(cache.denoms)
}

pub(crate) fn store_coverable_denoms(key: &CovCoverableKey<'_>, denoms: &[CoverableDenom]) {
    let Some(fingerprint) = coverable_fingerprint(key) else {
        return;
    };
    let cache = CovCoverableCache {
        schema_version: SCHEMA_VERSION.to_string(),
        fingerprint,
        denoms: denoms.to_vec(),
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

pub(crate) fn load_or_build_coverable_denoms(
    key: &CovCoverableKey<'_>,
) -> Result<Vec<CoverableDenom>, kiss::code_roles::RoleBuildError> {
    if let Some(cached) = try_load_coverable_denoms(key) {
        return Ok(cached);
    }
    let facts = CoverageSourceFacts::from_files(key.py_files, key.rs_files)?;
    let denoms = facts.production_denoms();
    store_coverable_denoms(key, &denoms);
    Ok(denoms)
}

fn cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("cov_coverable_cache.json")
}

fn coverable_fingerprint(key: &CovCoverableKey<'_>) -> Option<String> {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, SCHEMA_VERSION.as_bytes());
    if let Some(lang) = key.lang_filter {
        h = fnv1a64(h, lang.as_bytes());
    }
    for prefix in key.ignore {
        h = fnv1a64(h, prefix.as_bytes());
        h = fnv1a64(h, &[0]);
    }
    h = crate::analyze_cache::mix_sorted_paths_len_mtime(
        h,
        key.py_files.iter().chain(key.rs_files),
    );
    Some(format!("{h:016x}"))
}

#[cfg(test)]
#[path = "cov_coverable_cache_test.rs"]
mod tests;
