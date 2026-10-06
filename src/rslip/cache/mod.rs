use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::fs::{File, OpenOptions};

use crate::rpytest_runner::TestStatus;

use crate::rslip::{CACHE_SCHEMA_VERSION, LineCoverage, RslipOutcome, RslipRequest};
mod memo;
mod record;
mod statement_digest;
pub(crate) use memo::{DigestMemo, load_reusable_rslip_cache_entry_with_memo};
use record::deps_still_hold;
pub use record::python_records_dir;
pub(crate) use record::{
    load_reusable_rslip_cache_entry, load_rslip_cache_entry, load_rslip_record,
    store_rslip_cache_entry,
};

#[derive(Clone, Debug)]
pub(crate) struct RslipCacheEntry {
    pub(crate) nodeid: String,
    pub(crate) status: TestStatus,
    pub(crate) exit_code: Option<i32>,
    pub(crate) duration: std::time::Duration,
    pub(crate) coverage: LineCoverage,
    pub(crate) covered_digests: BTreeMap<String, String>,
}

impl RslipCacheEntry {
    #[cfg(test)]
    pub(crate) fn from_outcome(outcome: &RslipOutcome, source_root: &Path) -> Self {
        let mut memo = DigestMemo::new();
        Self::from_outcome_with_memo(outcome, source_root, &mut memo)
    }

    pub(crate) fn from_outcome_with_memo(
        outcome: &RslipOutcome,
        source_root: &Path,
        memo: &mut DigestMemo,
    ) -> Self {
        Self {
            nodeid: outcome.nodeid.clone(),
            status: outcome.status,
            exit_code: outcome.exit_code,
            duration: outcome.duration,
            coverage: outcome.coverage.clone(),
            covered_digests: memo::covered_file_digests_with_memo(
                source_root,
                &outcome.nodeid,
                &outcome.coverage,
                memo,
            )
            .unwrap_or_default(),
        }
    }
}

pub(crate) fn entry_is_reusable(entry: &RslipCacheEntry, source_root: &Path) -> bool {
    has_dependency_evidence(entry)
        && deps_still_hold(
            entry,
            covered_file_digests(source_root, &entry.nodeid, &entry.coverage).as_ref(),
        )
}

fn has_dependency_evidence(entry: &RslipCacheEntry) -> bool {
    !entry.coverage.files.is_empty()
}

#[cfg(test)]
pub(crate) fn create_new_rslip_cache_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
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
    h = rslip_fnv1a64(h, req.cache_root.to_string_lossy().as_bytes());
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
    memo::covered_file_digests_with_memo(source_root, nodeid, coverage, &mut DigestMemo::new())
}

pub(super) fn uses_statement_granularity(recorded: &str, test_module: &str) -> bool {
    recorded != test_module && recorded.ends_with(".py")
}

pub(super) fn read_recorded_text(source_root: &Path, recorded: &str) -> Option<String> {
    let bytes = fs::read(resolve_recorded_path(source_root, recorded)).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn module_digest_only(source_root: &Path, nodeid: &str) -> Option<BTreeMap<String, String>> {
    let module = test_module_path_from_nodeid(nodeid);
    if module.is_empty() || is_non_digestable_coverage_path(module) {
        return None;
    }
    let digest = digest_recorded_path(source_root, module)?;
    let mut digests = BTreeMap::new();
    digests.insert(module.to_string(), digest);
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

const CHANGED_DURING_RUN: &str = "changed-during-run:";

pub(crate) fn mark_deps_changed_during_run(
    source_root: &Path,
    deps: &mut BTreeMap<String, String>,
    since: std::time::SystemTime,
    prior: Option<&BTreeMap<String, String>>,
) {
    for (recorded, digest) in deps.iter_mut() {
        if !modified_after(source_root, recorded, since) {
            continue;
        }
        let marked = format!("{CHANGED_DURING_RUN}{digest}");
        if prior.and_then(|prior| prior.get(recorded)) != Some(&marked) {
            *digest = marked;
        }
    }
}

fn modified_after(source_root: &Path, recorded: &str, since: std::time::SystemTime) -> bool {
    fs::metadata(resolve_recorded_path(source_root, recorded))
        .and_then(|meta| meta.modified())
        .map_or(true, |modified| modified > since)
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
    let test_dir = path.parent();
    file_name_str(Some(path)) == Some("rslip_cache")
        && file_name_str(test_dir) == Some("test")
        && file_name_str(test_dir.and_then(Path::parent)) == Some(".kiss")
}

fn file_name_str(path: Option<&Path>) -> Option<&str> {
    path?.file_name()?.to_str()
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
