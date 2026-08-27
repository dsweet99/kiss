use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::runners::rust_logical_to_kiss_test_ids;
use super::workspace_selector_cache::{normalized_root, workspace_files_fingerprint_for_cache};

const SCHEMA_VERSION: &str = "rust-test-report-ids-v1";
const CACHE_FILE_NAME: &str = "rust_test_report_ids.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RustReportIdCache {
    schema_version: String,
    source_root: String,
    ignore: Vec<String>,
    files_fingerprint: String,
    report_ids: BTreeMap<String, String>,
}

fn cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join(CACHE_FILE_NAME)
}

type ReportIdMemoEntry = (String, Vec<String>, BTreeMap<String, String>);

static IN_PROCESS_MEMO: Mutex<Option<ReportIdMemoEntry>> = Mutex::new(None);

pub(crate) fn rust_logical_to_kiss_test_ids_cached(
    repo_root: &Path,
    ignore: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let source_root = normalized_root(repo_root);
    if let Ok(memo) = IN_PROCESS_MEMO.lock()
        && let Some((root, ign, map)) = memo.as_ref()
        && root == &source_root
        && ign == ignore
    {
        return Ok(map.clone());
    }
    let map = if let Some(map) = try_load_cached(repo_root, ignore) {
        map
    } else {
        let map = rust_logical_to_kiss_test_ids(repo_root, ignore)?;
        let _ = store_cached(repo_root, ignore, &map);
        map
    };
    if let Ok(mut memo) = IN_PROCESS_MEMO.lock() {
        *memo = Some((source_root, ignore.to_vec(), map.clone()));
    }
    Ok(map)
}

fn try_load_cached(repo_root: &Path, ignore: &[String]) -> Option<BTreeMap<String, String>> {
    let bytes = fs::read(cache_path(repo_root)).ok()?;
    let cache: RustReportIdCache = serde_json::from_slice(&bytes).ok()?;
    if cache.schema_version != SCHEMA_VERSION
        || cache.source_root != normalized_root(repo_root)
        || cache.ignore != ignore
    {
        return None;
    }
    let files_fingerprint = workspace_files_fingerprint_for_cache(repo_root, ignore).ok()?;
    if cache.files_fingerprint != files_fingerprint {
        return None;
    }
    Some(cache.report_ids)
}

fn store_cached(
    repo_root: &Path,
    ignore: &[String],
    report_ids: &BTreeMap<String, String>,
) -> io::Result<()> {
    let files_fingerprint = workspace_files_fingerprint_for_cache(repo_root, ignore)?;
    let cache = RustReportIdCache {
        schema_version: SCHEMA_VERSION.to_string(),
        source_root: normalized_root(repo_root),
        ignore: ignore.to_vec(),
        files_fingerprint,
        report_ids: report_ids.clone(),
    };
    let path = cache_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name(format!(
        ".{CACHE_FILE_NAME}.{}.tmp",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let mut file = File::create(&tmp)?;
    serde_json::to_writer(&mut file, &cache).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
#[path = "rust_report_id_cache_test.rs"]
mod tests;
