use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::plan::batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity};
use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::plan::shared_input::rust_cov_input_files;

const SEAL_SCHEMA_VERSION: &str = "rust-input-mtime-seal-v2";
const SEAL_FILE_NAME: &str = "input_mtime_seal.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SealFileMeta {
    path: String,
    len: u64,
    mtime_ns: u64,
    /// Unix ctime (status-change time). Content writes and mtime restores via
    /// `utimensat` update ctime even when mtime is forced back, so same-length
    /// rewrites with preserved mtime still miss the seal.
    ctime_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct InputMtimeSeal {
    schema_version: String,
    source_root: String,
    runner_map_fingerprint: String,
    cargo_version: String,
    llvm_cov_version: String,
    rustc_version: String,
    cargo_nextest_version: String,
    input_digest: String,
    generation_fingerprint: String,
    selection_context_fingerprint: String,
    ordinary_source_digests: BTreeMap<String, String>,
    files: Vec<SealFileMeta>,
}

fn seal_path(cache_root: &Path) -> PathBuf {
    cache_root.join(SEAL_FILE_NAME)
}

fn mtime_ns(meta: &fs::Metadata) -> Option<u64> {
    let modified = meta.modified().ok()?;
    Some(
        modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    )
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

fn collect_file_meta(source_root: &Path) -> io::Result<Vec<SealFileMeta>> {
    let files = rust_cov_input_files(source_root)?;
    let root = source_root
        .canonicalize()
        .unwrap_or_else(|_| source_root.to_path_buf());
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let meta = fs::metadata(&file)?;
        let rel = file
            .strip_prefix(&root)
            .or_else(|_| file.strip_prefix(source_root))
            .map_err(|_| {
                io::Error::other(format!(
                    "input path is not repository-relative: {}",
                    file.display()
                ))
            })?
            .to_string_lossy()
            .replace('\\', "/");
        out.push(SealFileMeta {
            path: rel,
            len: meta.len(),
            mtime_ns: mtime_ns(&meta).unwrap_or(0),
            ctime_ns: ctime_ns(&meta),
        });
    }
    Ok(out)
}

fn file_meta_matches(source_root: &Path, expected: &[SealFileMeta]) -> bool {
    let Ok(current) = collect_file_meta(source_root) else {
        return false;
    };
    current == expected
}

/// Persist a content-identity seal keyed by input-file size/mtime/ctime metadata.
/// Written by `kiss cov` publish so the next `kiss test` can skip re-hashing.
pub fn write_identity_mtime_seal(
    cache_root: &Path,
    source_root: &Path,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> io::Result<()> {
    let seal = InputMtimeSeal {
        schema_version: SEAL_SCHEMA_VERSION.to_string(),
        source_root: crate::rust_cov_cache::normalized_source_root(source_root),
        runner_map_fingerprint: req.runner_map_fingerprint.clone(),
        cargo_version: tools.cargo_version.clone(),
        llvm_cov_version: tools.llvm_cov_version.clone(),
        rustc_version: tools.rustc_version.clone(),
        cargo_nextest_version: tools.cargo_nextest_version.clone(),
        input_digest: identity.input_digest.clone(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        ordinary_source_digests: identity.ordinary_source_digests.clone(),
        files: collect_file_meta(source_root)?,
    };
    let path = seal_path(cache_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = parent_tmp_path(&path)?;
    let mut file = crate::rust_cov_cache::create_new_cache_file(&tmp)?;
    serde_json::to_writer(&mut file, &seal).map_err(io::Error::other)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn parent_tmp_path(path: &Path) -> io::Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("seal path has no parent"))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok(parent.join(format!(".input_mtime_seal.{nanos}.tmp")))
}

/// Fast path: reuse digests when input-file size/mtime/ctime set and tool identity match.
pub fn try_identity_from_mtime_seal(
    cache_root: &Path,
    source_root: &Path,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> Option<RustCoverageBatchIdentity> {
    let bytes = fs::read(seal_path(cache_root)).ok()?;
    let seal: InputMtimeSeal = serde_json::from_slice(&bytes).ok()?;
    if seal.schema_version != SEAL_SCHEMA_VERSION {
        return None;
    }
    if seal.source_root != crate::rust_cov_cache::normalized_source_root(source_root) {
        return None;
    }
    if seal.runner_map_fingerprint != req.runner_map_fingerprint
        || seal.cargo_version != tools.cargo_version
        || seal.llvm_cov_version != tools.llvm_cov_version
        || seal.rustc_version != tools.rustc_version
        || seal.cargo_nextest_version != tools.cargo_nextest_version
    {
        return None;
    }
    if !file_meta_matches(source_root, &seal.files) {
        return None;
    }



    let live_generation = crate::plan::batch_fingerprint::generation_fingerprint(
        &seal.input_digest,
        req,
        tools,
        crate::BATCH_EXECUTION_POLICY_VERSION,
    );
    if live_generation != seal.generation_fingerprint {
        return None;
    }
    Some(RustCoverageBatchIdentity {
        input_digest: seal.input_digest,
        generation_fingerprint: seal.generation_fingerprint,
        selection_context_fingerprint: seal.selection_context_fingerprint,
        ordinary_source_digests: seal.ordinary_source_digests,
    })
}
