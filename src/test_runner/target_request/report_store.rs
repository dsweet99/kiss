use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::digest::digest_bytes;
use super::report::TargetReport;
use super::types::TargetRequest;

const SCHEMA: &str = "target-report-store-v2";
const ENTRY_LIMIT: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredReport {
    seq: u64,
    key: String,
    report: TargetReport,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoreMeta {
    next_seq: u64,
    keys: Vec<String>,
}

pub(crate) fn publish_if_rows_hold(
    repo_root: &Path,
    request: &TargetRequest,
    report: &TargetReport,
) -> Result<(), String> {
    let recaptured = super::recapture::recapture_report(repo_root, request, report)?;
    publish_report(repo_root, request, &recaptured)
}

pub(crate) fn publish_report(
    repo_root: &Path,
    request: &TargetRequest,
    report: &TargetReport,
) -> Result<(), String> {
    let key = report_key(request, report);
    let dir = store_dir(repo_root);
    fs::create_dir_all(&dir).map_err(|err| format!("target report store: {err}"))?;
    let (seq, observed) = {
        let _lock = lock_store(&dir)?;
        let observed = read_key_pointer(&dir, &key).map(|stored| stored.seq);
        let mut meta = read_meta(&dir);
        let seq = meta.next_seq;
        meta.next_seq += 1;
        write_meta(&dir, &meta)?;
        (seq, observed)
    };
    let entry = StoredReport {
        seq,
        key: key.clone(),
        report: report.clone(),
    };
    write_entry(&dir, &entry)?;
    let _lock = lock_store(&dir)?;
    if read_key_pointer(&dir, &key)
        .as_ref()
        .map(|stored| stored.seq)
        != observed
    {
        return Err("concurrent mutation".into());
    }
    let mut meta = read_meta(&dir);
    meta.keys.retain(|item| item != &key);
    meta.keys.push(key);
    prune_unlocked(&dir, &mut meta);
    write_meta(&dir, &meta)?;
    write_key_pointer(&dir, &entry)?;
    write_pointer(&dir, &entry)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn store_present(repo_root: &Path) -> bool {
    let dir = store_dir(repo_root);
    dir.join("pointer.json").is_file() || dir.join("pointers").is_dir()
}

pub(crate) fn load_current_report(repo_root: &Path) -> Option<TargetReport> {
    read_pointer(&store_dir(repo_root)).map(|stored| stored.report)
}

pub(crate) fn load_report_for_identity(
    repo_root: &Path,
    request: &TargetRequest,
    digest: &str,
    complete: bool,
    coverage_all: bool,
    extra: &[String],
) -> Option<TargetReport> {
    let (runner, gate_policy) = super::report::evaluation_key_tokens(repo_root, coverage_all);
    let key = identity_key(request, digest, complete, &runner, &gate_policy, extra);
    read_key_pointer(&store_dir(repo_root), &key).map(|stored| stored.report)
}

fn report_key(request: &TargetRequest, report: &TargetReport) -> String {
    identity_key(
        request,
        &report.stamp.digest,
        report.stamp.complete,
        &report.snapshot.runner,
        &report.snapshot.gate_policy,
        &report.snapshot.extra,
    )
}

fn identity_key(
    request: &TargetRequest,
    digest: &str,
    complete: bool,
    runner: &str,
    gate_policy: &str,
    extra: &[String],
) -> String {
    let mut payload = serde_json::json!({
        "schema": SCHEMA,
        "request": request,
        "digest": digest,
        "complete": complete,
        "runner": runner,
        "gate_policy": gate_policy,
    });
    if !extra.is_empty() {
        payload["extra"] = serde_json::json!(extra);
    }
    digest_bytes(&serde_json::to_vec(&payload).expect("report key"))
}

fn store_dir(repo_root: &Path) -> PathBuf {
    repo_root
        .join("target")
        .join("kiss-plan")
        .join("target-reports")
}

fn lock_store(dir: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("lock"))
        .map_err(|err| format!("target report lock: {err}"))?;
    file.lock_exclusive()
        .map_err(|err| format!("target report lock: {err}"))?;
    Ok(file)
}

fn read_meta(dir: &Path) -> StoreMeta {
    fs::read(dir.join("meta.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_meta(dir: &Path, meta: &StoreMeta) -> Result<(), String> {
    atomic_json(dir, "meta.json", meta)
}

fn read_pointer(dir: &Path) -> Option<StoredReport> {
    let bytes = fs::read(dir.join("pointer.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_pointer(dir: &Path, entry: &StoredReport) -> Result<(), String> {
    atomic_json(dir, "pointer.json", entry)
}

fn key_pointer_name(key: &str) -> String {
    format!("pointers/{}.json", &key[..16.min(key.len())])
}

fn read_key_pointer(dir: &Path, key: &str) -> Option<StoredReport> {
    let bytes = fs::read(dir.join(key_pointer_name(key))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_key_pointer(dir: &Path, entry: &StoredReport) -> Result<(), String> {
    atomic_json(dir, &key_pointer_name(&entry.key), entry)
}

fn write_entry(dir: &Path, entry: &StoredReport) -> Result<(), String> {
    let name = format!(
        "entries/{:08}_{}.json",
        entry.seq,
        &entry.key[..16.min(entry.key.len())]
    );
    if let Some(parent) = dir.join(&name).parent() {
        fs::create_dir_all(parent).map_err(|err| format!("target report entry: {err}"))?;
    }
    atomic_json(dir, &name, entry)
}

fn prune_unlocked(dir: &Path, meta: &mut StoreMeta) {
    while meta.keys.len() > ENTRY_LIMIT {
        let oldest = meta.keys.remove(0);
        let prefix = &oldest[..16.min(oldest.len())];
        if let Ok(entries) = fs::read_dir(dir.join("entries")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.contains(prefix) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        let _ = fs::remove_file(dir.join(key_pointer_name(&oldest)));
    }
}

fn atomic_json<T: Serialize>(dir: &Path, name: &str, value: &T) -> Result<(), String> {
    let path = dir.join(name);
    let tmp = dir.join(format!(".{}.tmp", name.replace('/', "_")));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("target report write: {err}"))?;
    }
    let bytes = serde_json::to_vec(value).map_err(|err| format!("target report json: {err}"))?;
    {
        let mut file = File::create(&tmp).map_err(|err| format!("target report write: {err}"))?;
        file.write_all(&bytes)
            .map_err(|err| format!("target report write: {err}"))?;
    }
    fs::rename(tmp, path).map_err(|err| format!("target report publish: {err}"))
}
