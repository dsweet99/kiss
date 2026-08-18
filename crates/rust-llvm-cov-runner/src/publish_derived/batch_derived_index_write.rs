//! Atomic write for derived `index.json`.

use crate::RustLlvmCovError;
use crate::publish_derived::batch_derived::INDEX_SCHEMA_VERSION;
use crate::rust_cov_cache::rust_cov_unique_suffix;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
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
    let tmp_path = parent.join(format!(".index.{}.tmp", rust_cov_unique_suffix()));
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
    kiss_publication_barrier::publish_atomically("rust_derived_index", &path, &tmp_path, |file| {
        serde_json::to_writer(&mut *file, &payload).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        Ok(())
    })
    .map_err(RustLlvmCovError::Io)
}
