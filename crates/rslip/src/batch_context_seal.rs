//! Best-effort mtime seal for rslip batch context (not used by fingerprint unit API).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::RslipRequest;
use crate::cache::rslip_input_files;

const SEAL_SCHEMA_VERSION: &str = "rslip-batch-mtime-seal-v1";
const SEAL_FILE_NAME: &str = "batch_input_mtime_seal.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SealFileMeta {
    path: String,
    len: u64,
    mtime_ns: u64,
    ctime_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct InputMtimeSeal {
    schema_version: String,
    source_root: String,
    python_version: String,
    pytest_version: String,
    pytest_args: Vec<String>,
    env: BTreeMap<String, String>,
    context_fingerprint: String,
    files: Vec<SealFileMeta>,
}

fn seal_path(cache_root: &Path) -> PathBuf {
    cache_root.join(SEAL_FILE_NAME)
}

fn mtime_ns(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn ctime_ns(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.ctime() as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(meta.ctime_nsec() as u64)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

fn collect_file_meta(root: &Path) -> io::Result<Vec<SealFileMeta>> {
    let files = rslip_input_files(root)?;
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let meta = fs::metadata(&file)?;
        let rel = file
            .strip_prefix(root)
            .map_err(|_| {
                io::Error::other(format!(
                    "rslip input path is not repository-relative: {}",
                    file.display()
                ))
            })?
            .to_string_lossy()
            .replace('\\', "/");
        out.push(SealFileMeta {
            path: rel,
            len: meta.len(),
            mtime_ns: mtime_ns(&meta),
            ctime_ns: ctime_ns(&meta),
        });
    }
    Ok(out)
}

pub(crate) fn try_batch_context_seal(req: &RslipRequest) -> Option<String> {
    let bytes = fs::read(seal_path(&req.cache_root)).ok()?;
    let seal: InputMtimeSeal = serde_json::from_slice(&bytes).ok()?;
    if seal.schema_version != SEAL_SCHEMA_VERSION {
        return None;
    }
    let root = req.cwd.canonicalize().ok()?;
    if seal.source_root != root.to_string_lossy()
        || seal.python_version != req.python_version
        || seal.pytest_version != req.pytest_version
        || seal.pytest_args != req.pytest_args
        || seal.env != req.env
    {
        return None;
    }
    let current = collect_file_meta(&root).ok()?;
    (current == seal.files).then_some(seal.context_fingerprint)
}

pub(crate) fn write_batch_context_seal(
    req: &RslipRequest,
    context_fingerprint: &str,
) -> io::Result<()> {
    let root = req.cwd.canonicalize()?;
    let seal = InputMtimeSeal {
        schema_version: SEAL_SCHEMA_VERSION.to_string(),
        source_root: root.to_string_lossy().to_string(),
        python_version: req.python_version.clone(),
        pytest_version: req.pytest_version.clone(),
        pytest_args: req.pytest_args.clone(),
        env: req.env.clone(),
        context_fingerprint: context_fingerprint.to_string(),
        files: collect_file_meta(&root)?,
    };
    let path = seal_path(&req.cache_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp.{nanos}"));
    let mut file = File::create(&tmp)?;
    serde_json::to_writer(&mut file, &seal).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}
