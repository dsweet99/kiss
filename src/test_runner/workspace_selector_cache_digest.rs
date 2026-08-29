use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::analyze_cache::fnv1a64;

const DISK_SCHEMA: &str = "selector-source-digests-v2";
const DISK_FILE: &str = "selector_source_digests.json";

#[derive(Clone, Eq, PartialEq, Hash)]
struct FileStamp {
    path: PathBuf,
    len: u64,
    mtime_ns: u64,
    ctime_ns: u64,
    dev: u64,
    ino: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct DiskRecord {
    len: u64,
    mtime_ns: u64,
    ctime_ns: u64,
    dev: u64,
    ino: u64,
    digest: u64,
}

#[derive(Serialize, Deserialize)]
struct DiskFile {
    schema_version: String,
    files: BTreeMap<String, DiskRecord>,
}

fn memo() -> &'static Mutex<HashMap<FileStamp, u64>> {
    static MEMO: OnceLock<Mutex<HashMap<FileStamp, u64>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

fn disk_maps() -> &'static Mutex<HashMap<PathBuf, BTreeMap<String, DiskRecord>>> {
    static MAPS: OnceLock<Mutex<HashMap<PathBuf, BTreeMap<String, DiskRecord>>>> = OnceLock::new();
    MAPS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn system_time_ns(ts: SystemTime) -> u64 {
    ts.duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn ctime_ns(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let sec = u64::try_from(meta.ctime()).unwrap_or(0);
        let nsec = u64::try_from(meta.ctime_nsec()).unwrap_or(0);
        sec.saturating_mul(1_000_000_000).saturating_add(nsec)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

fn device_inode(meta: &fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        (0, 0)
    }
}

fn stamp_for(path: &Path, meta: &fs::Metadata) -> FileStamp {
    let (dev, ino) = device_inode(meta);
    FileStamp {
        path: path.to_path_buf(),
        len: meta.len(),
        mtime_ns: meta.modified().map(system_time_ns).unwrap_or(0),
        ctime_ns: ctime_ns(meta),
        dev,
        ino,
    }
}

fn record_matches(record: &DiskRecord, stamp: &FileStamp) -> bool {
    record.len == stamp.len
        && record.mtime_ns == stamp.mtime_ns
        && record.ctime_ns == stamp.ctime_ns
        && record.dev == stamp.dev
        && record.ino == stamp.ino
}

fn load_disk_map(repo_root: &Path) -> BTreeMap<String, DiskRecord> {
    let path = repo_root.join(".kiss").join(DISK_FILE);
    let Ok(bytes) = fs::read(path) else {
        return BTreeMap::new();
    };
    let Ok(parsed) = serde_json::from_slice::<DiskFile>(&bytes) else {
        return BTreeMap::new();
    };
    if parsed.schema_version != DISK_SCHEMA {
        return BTreeMap::new();
    }
    parsed.files
}

fn repo_disk_map(repo_root: &Path) -> BTreeMap<String, DiskRecord> {
    let key = repo_root.to_path_buf();
    if let Ok(guard) = disk_maps().lock()
        && let Some(map) = guard.get(&key)
    {
        return map.clone();
    }
    let loaded = load_disk_map(repo_root);
    if let Ok(mut guard) = disk_maps().lock() {
        guard.insert(key, loaded.clone());
    }
    loaded
}

fn remember_disk_record(repo_root: &Path, rel: &str, stamp: &FileStamp, digest: u64) {
    let key = repo_root.to_path_buf();
    let record = DiskRecord {
        len: stamp.len,
        mtime_ns: stamp.mtime_ns,
        ctime_ns: stamp.ctime_ns,
        dev: stamp.dev,
        ino: stamp.ino,
        digest,
    };
    if let Ok(mut guard) = disk_maps().lock() {
        guard
            .entry(key)
            .or_default()
            .insert(rel.to_string(), record);
    }
}

fn content_digest(repo_root: &Path, rel: &str, path: &Path, stamp: &FileStamp) -> io::Result<u64> {
    if let Ok(guard) = memo().lock()
        && let Some(digest) = guard.get(stamp).copied()
    {
        return Ok(digest);
    }
    let disk = repo_disk_map(repo_root);
    if let Some(record) = disk.get(rel)
        && record_matches(record, stamp)
    {
        if let Ok(mut guard) = memo().lock() {
            guard.insert(stamp.clone(), record.digest);
        }
        return Ok(record.digest);
    }
    let bytes = fs::read(path)?;
    let hashed = if rel.ends_with(".rs") {
        rust_selector_declaration_bytes(&bytes)
    } else {
        bytes
    };
    let digest = fnv1a64(0xcbf2_9ce4_8422_2325, &hashed);
    if let Ok(mut guard) = memo().lock() {
        guard.insert(stamp.clone(), digest);
    }
    remember_disk_record(repo_root, rel, stamp, digest);
    Ok(digest)
}

pub(super) fn hash_file_contents(
    h: u64,
    rel: &str,
    repo_root: &Path,
    path: &Path,
) -> io::Result<u64> {
    let meta = fs::metadata(path)?;
    let stamp = stamp_for(path, &meta);
    let digest = content_digest(repo_root, rel, path, &stamp)?;
    let acc = fnv1a64(h, rel.as_bytes());
    Ok(fnv1a64(acc, &digest.to_le_bytes()))
}

pub(super) fn flush_persisted_digests(repo_root: &Path) {
    let key = repo_root.to_path_buf();
    let Some(files) = disk_maps()
        .lock()
        .ok()
        .and_then(|guard| guard.get(&key).cloned())
    else {
        return;
    };
    let dir = repo_root.join(".kiss");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let body = DiskFile {
        schema_version: DISK_SCHEMA.to_string(),
        files,
    };
    let Ok(bytes) = serde_json::to_vec(&body) else {
        return;
    };
    let _ = fs::write(dir.join(DISK_FILE), bytes);
}

fn rust_selector_declaration_bytes(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    if rust_signature_ambiguous(&text) {
        return bytes.to_vec();
    }
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(sig) = rust_declaration_signature(trimmed) {
            out.extend_from_slice(sig.as_bytes());
            out.push(b'\n');
        }
    }
    out
}

fn rust_signature_ambiguous(text: &str) -> bool {
    text.contains("proc_macro") || text.contains("include!") || text.contains("macro_rules!")
}

fn rust_declaration_signature(line: &str) -> Option<&str> {
    if line.starts_with("#[") {
        return Some(line);
    }
    if rust_declaration_line(line) {
        return Some(line.split_once('{').map_or(line, |(sig, _)| sig).trim_end());
    }
    None
}

fn rust_declaration_line(line: &str) -> bool {
    line.starts_with("fn ")
        || line.starts_with("pub fn ")
        || line.starts_with("pub(crate) fn ")
        || line.starts_with("async fn ")
        || line.starts_with("pub async fn ")
        || line.starts_with("mod ")
        || line.starts_with("pub mod ")
}
