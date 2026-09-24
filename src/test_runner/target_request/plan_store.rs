use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::digest::digest_bytes;
use super::slice::TargetSliceStamp;
use super::types::TargetRequest;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

pub(crate) const TARGET_PLAN_ENTRY_LIMIT: usize = 64;
const SCHEMA: &str = "target-plan-store-v2";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TargetPlanEntry {
    pub seq: u64,
    pub key: String,
    pub stamp: TargetSliceStamp,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoreMeta {
    next_seq: u64,
    entries: Vec<TargetPlanEntry>,
}

pub(crate) fn publish(
    repo_root: &Path,
    request: &TargetRequest,
    stamp: &TargetSliceStamp,
) -> Result<TargetPlanEntry, String> {
    let key = plan_key(request, stamp, &super::report::runner_identity(repo_root));
    let dir = store_dir(repo_root);
    fs::create_dir_all(&dir).map_err(|err| format!("target plan store: {err}"))?;
    let lock = lock_store(&dir)?;
    let mut meta = read_meta(&dir);
    if let Some(existing) = meta.entries.iter().find(|entry| entry.key == key).cloned() {
        write_pointer(&dir, &existing)?;
        drop(lock);
        return Ok(existing);
    }
    let entry = TargetPlanEntry {
        seq: meta.next_seq,
        key,
        stamp: stamp.clone(),
    };
    meta.next_seq += 1;
    write_entry(&dir, &entry)?;
    meta.entries.push(entry.clone());
    prune_unlocked(&dir, &mut meta);
    write_meta(&dir, &meta)?;
    write_pointer(&dir, &entry)?;
    drop(lock);
    Ok(entry)
}

pub(crate) fn load_current(repo_root: &Path) -> Option<TargetPlanEntry> {
    let dir = store_dir(repo_root);
    let bytes = fs::read(dir.join("pointer.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) fn load_entry(repo_root: &Path, key: &str) -> Option<TargetPlanEntry> {
    let dir = store_dir(repo_root);
    let meta = read_meta(&dir);
    meta.entries.into_iter().find(|entry| entry.key == key)
}

pub(crate) fn load_plan_for_identity(
    repo_root: &Path,
    request: &TargetRequest,
    stamp: &TargetSliceStamp,
) -> Option<TargetPlanEntry> {
    let key = plan_key(request, stamp, &super::report::runner_identity(repo_root));
    load_entry(repo_root, &key)
}

fn plan_key(request: &TargetRequest, stamp: &TargetSliceStamp, runner: &str) -> String {
    let payload = serde_json::json!({
        "schema": SCHEMA,
        "request": request,
        "digest": stamp.digest,
        "complete": stamp.complete,
        "runner": runner,
    });
    digest_bytes(&serde_json::to_vec(&payload).expect("plan key"))
}

#[cfg(test)]
pub(crate) fn plan_key_for_test(
    request: &TargetRequest,
    stamp: &TargetSliceStamp,
    runner: &str,
) -> String {
    plan_key(request, stamp, runner)
}

fn store_dir(repo_root: &Path) -> PathBuf {
    repo_root
        .join("target")
        .join("kiss-plan")
        .join("target-plans")
}

fn lock_store(dir: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("lock"))
        .map_err(|err| format!("target plan lock: {err}"))?;
    file.lock_exclusive()
        .map_err(|err| format!("target plan lock: {err}"))?;
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

fn write_pointer(dir: &Path, entry: &TargetPlanEntry) -> Result<(), String> {
    atomic_json(dir, "pointer.json", entry)
}

fn write_entry(dir: &Path, entry: &TargetPlanEntry) -> Result<(), String> {
    let name = format!(
        "entries/{:08}_{}.json",
        entry.seq,
        &entry.key[..16.min(entry.key.len())]
    );
    if let Some(parent) = dir.join(&name).parent() {
        fs::create_dir_all(parent).map_err(|err| format!("target plan entry: {err}"))?;
    }
    atomic_json(dir, &name, entry)
}

fn prune_unlocked(dir: &Path, meta: &mut StoreMeta) {
    while meta.entries.len() > TARGET_PLAN_ENTRY_LIMIT {
        let oldest = meta.entries.remove(0);
        let name = format!(
            "entries/{:08}_{}.json",
            oldest.seq,
            &oldest.key[..16.min(oldest.key.len())]
        );
        let _ = fs::remove_file(dir.join(name));
    }
}

fn atomic_json<T: Serialize>(dir: &Path, name: &str, value: &T) -> Result<(), String> {
    let path = dir.join(name);
    let tmp = dir.join(format!(".{}.tmp", name.replace('/', "_")));
    if let Some(parent) = tmp.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("target plan write: {err}"))?;
    }
    let bytes = serde_json::to_vec(value).map_err(|err| format!("target plan json: {err}"))?;
    {
        let mut file = File::create(&tmp).map_err(|err| format!("target plan write: {err}"))?;
        file.write_all(&bytes)
            .map_err(|err| format!("target plan write: {err}"))?;
    }
    fs::rename(tmp, path).map_err(|err| format!("target plan publish: {err}"))
}
