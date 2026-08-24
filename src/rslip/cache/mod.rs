use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::fs::{File, OpenOptions};

use crate::rpytest_runner::TestStatus;
use serde::{Deserialize, Serialize};

use crate::rslip::{CACHE_SCHEMA_VERSION, LineCoverage, RslipOutcome, RslipRequest};

mod memo;
pub(crate) use memo::{DigestMemo, load_reusable_rslip_cache_entry_with_memo};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RslipCacheEntry {
    schema_version: String,
    pub(crate) nodeid: String,
    pub(crate) status: TestStatus,
    pub(crate) exit_code: Option<i32>,
    pub(crate) duration: std::time::Duration,
    pub(crate) coverage: LineCoverage,
    #[serde(default)]
    pub(crate) covered_digests: BTreeMap<String, String>,
}

impl RslipCacheEntry {
    pub(crate) fn from_outcome(outcome: &RslipOutcome, source_root: &Path) -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION.to_string(),
            nodeid: outcome.nodeid.clone(),
            status: outcome.status,
            exit_code: outcome.exit_code,
            duration: outcome.duration,
            coverage: outcome.coverage.clone(),
            covered_digests: covered_file_digests(source_root, &outcome.nodeid, &outcome.coverage)
                .unwrap_or_default(),
        }
    }
}

pub(crate) fn load_rslip_cache_entry(
    cache_root: &Path,
    fingerprint: &str,
) -> Option<RslipCacheEntry> {
    let path = rslip_cache_entry_path(cache_root, fingerprint);
    let bytes = fs::read(path).ok()?;
    let entry: RslipCacheEntry = serde_json::from_slice(&bytes).ok()?;
    (entry.schema_version == CACHE_SCHEMA_VERSION).then_some(entry)
}

pub(crate) fn load_reusable_rslip_cache_entry(
    cache_root: &Path,
    fingerprint: &str,
    source_root: &Path,
) -> Option<RslipCacheEntry> {
    let entry = load_rslip_cache_entry(cache_root, fingerprint)?;
    entry_is_reusable(&entry, source_root).then_some(entry)
}

pub(crate) fn entry_is_reusable(entry: &RslipCacheEntry, source_root: &Path) -> bool {
    if entry.status != crate::rpytest_runner::TestStatus::Passed {
        return false;
    }
    if entry.coverage.files.is_empty() {
        return false;
    }
    let Some(expected) = covered_file_digests(source_root, &entry.nodeid, &entry.coverage) else {
        return false;
    };
    expected == entry.covered_digests
}

pub(crate) fn store_rslip_cache_entry(
    cache_root: &Path,
    fingerprint: &str,
    entry: &RslipCacheEntry,
) -> io::Result<()> {
    let path = rslip_cache_entry_path(cache_root, fingerprint);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache path has no parent"))?;
    let tmp_path = parent.join(format!(".{}.{}.tmp", fingerprint, rslip_unique_suffix()));
    crate::kiss_publication_barrier::publish_atomically("rslip_selector_entry", &path, &tmp_path, |file| {
        serde_json::to_writer(&mut *file, entry).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        Ok(())
    })
}

#[cfg(test)]
pub(crate) fn create_new_rslip_cache_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

pub(crate) fn rslip_cache_entry_path(cache_root: &Path, fingerprint: &str) -> PathBuf {
    cache_root
        .join("entries")
        .join(format!("{fingerprint}.json"))
}

pub(crate) fn rslip_unique_suffix() -> String {
    crate::kiss_publication_barrier::unique_process_suffix()
}

pub(crate) fn rslip_cache_fingerprint(req: &RslipRequest) -> io::Result<String> {
    let context = rslip_request_context_fingerprint(req)?;
    Ok(rslip_cache_fingerprint_from_context(&context, &req.nodeid))
}

pub(crate) fn rslip_request_context_fingerprint(req: &RslipRequest) -> io::Result<String> {
    compute_rslip_request_context_fingerprint(req)
}

fn compute_rslip_request_context_fingerprint(req: &RslipRequest) -> io::Result<String> {
    let mut h = rslip_fnv1a64(0xcbf2_9ce4_8422_2325, CACHE_SCHEMA_VERSION.as_bytes());
    h = rslip_fnv1a64(h, req.python.to_string_lossy().as_bytes());
    h = rslip_fnv1a64(h, req.python_version.as_bytes());
    h = rslip_fnv1a64(h, req.pytest_version.as_bytes());
    h = rslip_fnv1a64(h, req.cwd.to_string_lossy().as_bytes());
    h = rslip_fnv1a64(h, req.source_root.to_string_lossy().as_bytes());
    for arg in &req.pytest_args {
        h = rslip_fnv1a64(h, arg.as_bytes());
        h = rslip_fnv1a64(h, &[0]);
    }
    for (key, value) in &req.env {
        h = rslip_fnv1a64(h, key.as_bytes());
        h = rslip_fnv1a64(h, b"=");
        h = rslip_fnv1a64(h, value.as_bytes());
        h = rslip_fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

pub(crate) fn rslip_cache_fingerprint_from_context(
    context_fingerprint: &str,
    nodeid: &str,
) -> String {
    let mut h = rslip_fnv1a64(0xcbf2_9ce4_8422_2325, CACHE_SCHEMA_VERSION.as_bytes());
    h = rslip_fnv1a64(h, context_fingerprint.as_bytes());
    h = rslip_fnv1a64(h, &[0]);
    h = rslip_fnv1a64(h, nodeid.as_bytes());
    format!("{h:016x}")
}

pub(crate) fn covered_file_digests(
    source_root: &Path,
    nodeid: &str,
    coverage: &LineCoverage,
) -> Option<BTreeMap<String, String>> {
    if coverage.files.is_empty() {
        return None;
    }
    let mut digests = BTreeMap::new();
    for recorded in coverage.files.keys() {
        if is_non_digestable_coverage_path(recorded) {
            continue;
        }
        let digest = digest_recorded_path(source_root, recorded)?;
        digests.insert(recorded.clone(), digest);
    }
    let module = test_module_path_from_nodeid(nodeid);
    if !module.is_empty()
        && !is_non_digestable_coverage_path(module)
        && let Some(digest) = digest_recorded_path(source_root, module)
    {
        digests.insert(module.to_string(), digest);
    }
    if digests.is_empty() {
        return Some(digests);
    }
    Some(digests)
}

pub(crate) fn test_module_path_from_nodeid(nodeid: &str) -> &str {
    nodeid.split_once("::").map_or(nodeid, |(module, _)| module)
}

pub(super) fn is_non_digestable_coverage_path(recorded: &str) -> bool {
    recorded.starts_with('<')
        || recorded.starts_with(".kiss/")
        || recorded.contains("rslip_runtime")
        || Path::new(recorded)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("[type "))
}

pub(crate) fn digest_recorded_path(source_root: &Path, recorded: &str) -> Option<String> {
    let path = resolve_recorded_path(source_root, recorded);
    let bytes = fs::read(path).ok()?;
    let h = rslip_fnv1a64(0xcbf2_9ce4_8422_2325, &bytes);
    Some(format!("{h:016x}"))
}

fn resolve_recorded_path(source_root: &Path, recorded: &str) -> PathBuf {
    let path = Path::new(recorded);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        source_root.join(path)
    }
}

#[cfg(test)]
pub(crate) fn rslip_input_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_rslip_inputs(root, &mut out)?;
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    Ok(out)
}

#[cfg(test)]
fn visit_rslip_inputs(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_skip_rslip_dir(&path) {
                continue;
            }
            visit_rslip_inputs(&path, out)?;
        } else if file_type.is_file() && is_rslip_cache_input(&path) {
            out.push(path);
        }
    }
    Ok(())
}

pub fn should_skip_rslip_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            ".git"
                | ".pytest_cache"
                | "__pycache__"
                | ".venv"
                | "venv"
                | "target"
                | ".rslip_cache"
                | ".kiss"
        )
    ) || is_kiss_rslip_cache_dir(path)
}

pub fn is_kiss_rslip_cache_dir(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("rslip_cache")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some(".kiss")
}

pub fn is_rslip_cache_input(path: &Path) -> bool {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        return true;
    }
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("pytest.ini" | "pyproject.toml" | "setup.cfg" | "tox.ini")
    )
}

pub(crate) fn rslip_fnv1a64(h: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0100_0000_01b3;
    bytes
        .iter()
        .fold(h, |acc, byte| (acc ^ u64::from(*byte)).wrapping_mul(PRIME))
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
