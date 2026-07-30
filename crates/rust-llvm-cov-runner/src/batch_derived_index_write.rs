//! Atomic write for derived `index.json`.

use crate::batch_derived::INDEX_SCHEMA_VERSION;
use crate::rust_cov_cache::{create_new_cache_file, rust_cov_unique_suffix};
use crate::RustLlvmCovError;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;

type RustCoverageIndex = BTreeMap<String, BTreeSet<String>>;

pub(crate) fn write_coverage_index(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
    entries_fingerprint: &str,
    index: &RustCoverageIndex,
) -> Result<(), RustLlvmCovError> {
    #[derive(Serialize)]
    struct OnDiskIndex<'a> {
        schema_version: &'a str,
        source_root: String,
        generation_fingerprint: &'a str,
        entries_fingerprint: &'a str,
        files: &'a RustCoverageIndex,
    }

    let path = cache_root.join("index.json");
    let parent = path
        .parent()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("index path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(RustLlvmCovError::Io)?;
    let tmp_path = parent.join(format!(".index.{}.tmp", rust_cov_unique_suffix()));
    let mut file = create_new_cache_file(&tmp_path).map_err(RustLlvmCovError::Io)?;
    let payload = OnDiskIndex {
        schema_version: INDEX_SCHEMA_VERSION,
        source_root: source_root
            .canonicalize()
            .unwrap_or_else(|_| source_root.to_path_buf())
            .to_string_lossy()
            .to_string(),
        generation_fingerprint: generation,
        entries_fingerprint,
        files: index,
    };
    serde_json::to_writer_pretty(&mut file, &payload).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("failed to write index json: {err}"))
    })?;
    file.write_all(b"\n").map_err(RustLlvmCovError::Io)?;
    file.sync_all().map_err(RustLlvmCovError::Io)?;
    kiss_publication_barrier::after_sync_before_rename("rust_derived_index", &tmp_path, &path)
        .map_err(RustLlvmCovError::Io)?;
    drop(file);
    fs::rename(&tmp_path, &path).map_err(RustLlvmCovError::Io)?;
    kiss_publication_barrier::after_rename("rust_derived_index", &tmp_path, &path)
        .map_err(RustLlvmCovError::Io)?;
    let dir = fs::File::open(parent).map_err(RustLlvmCovError::Io)?;
    dir.sync_all().map_err(RustLlvmCovError::Io)
}
