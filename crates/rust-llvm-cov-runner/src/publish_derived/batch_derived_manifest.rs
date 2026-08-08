use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use crate::publish_derived::batch_derived::POPULATION_SCHEMA_VERSION;
use crate::plan::batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity,
};
use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::publish_derived::batch_reverse_build::ReversePublishInfo;
use crate::rust_cov_cache::rust_cov_unique_suffix;
use crate::{CACHE_SCHEMA_VERSION, RustLlvmCovError, RustTestBinaryIdentity};

pub(crate) fn write_population_manifest(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
    test_binaries: &[RustTestBinaryIdentity],
    entries_fingerprint: &str,
    reverse: Option<&ReversePublishInfo>,
) -> Result<(), RustLlvmCovError> {
    let path = cache_root.join("population.json");
    let parent = path
        .parent()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("population path has no parent".into()))?;
    let tmp_path = parent.join(format!(".population.{}.tmp", rust_cov_unique_suffix()));
    let reverse_meta = reverse.map(|info| ReverseLineIndexManifestMeta {
        schema_version: info.schema_version.as_str(),
        snapshot_id: info.snapshot_id.as_str(),
        meta_digest: info.meta_digest.as_str(),
        entry_state_revision: info.entry_state_revision,
    });
    let payload = PopulationManifest {
        schema_version: POPULATION_SCHEMA_VERSION,
        cache_schema_version: CACHE_SCHEMA_VERSION,
        source_root: source_root
            .canonicalize()
            .unwrap_or_else(|_| source_root.to_path_buf())
            .to_string_lossy()
            .to_string(),
        generation_fingerprint: &identity.generation_fingerprint,
        input_fingerprint: &identity.input_digest,
        selection_context_fingerprint: &identity.selection_context_fingerprint,
        entries_fingerprint,
        selectors,
        ordinary_source_digests: ordinary_source_digest_records(identity),
        test_binaries: test_binary_records(test_binaries),
        reverse_line_index: reverse_meta,
    };
    // Concurrent orphan-tmp sweep or tree replacement can yield NotFound mid-publish.
    // Retry once with a fresh tmp name.
    let mut last_err = None;
    for attempt in 0..2 {
        let attempt_tmp = if attempt == 0 {
            tmp_path.clone()
        } else {
            parent.join(format!(".population.{}.tmp", rust_cov_unique_suffix()))
        };
        match kiss_publication_barrier::publish_atomically(
            "rust_population",
            &path,
            &attempt_tmp,
            |file| {
                serde_json::to_writer_pretty(&mut *file, &payload).map_err(io::Error::other)?;
                file.write_all(b"\n")?;
                Ok(())
            },
        ) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound && attempt == 0 => {
                last_err = Some(err);
            }
            Err(err) => return Err(RustLlvmCovError::Io(err)),
        }
    }
    Err(RustLlvmCovError::Io(last_err.expect("NotFound retry path")))
}

/// Write `population.json`, then best-effort publish the durations sidecar.
pub(crate) fn write_population_and_durations(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
    test_binaries: &[RustTestBinaryIdentity],
    entries_fingerprint: &str,
    reverse: Option<&ReversePublishInfo>,
) -> Result<(), RustLlvmCovError> {
    write_population_manifest(
        &req.cache_root,
        &req.source_root,
        identity,
        selectors,
        test_binaries,
        entries_fingerprint,
        reverse,
    )?;
    crate::publish_derived::batch_population_durations::try_publish_durations_after_population(
        &req.cache_root,
        identity,
        req,
        tools,
        selectors,
        entries_fingerprint,
    );
    Ok(())
}

#[derive(Serialize)]
struct OrdinarySourceDigestRecord<'a> {
    path: &'a str,
    digest: &'a str,
}

#[derive(Serialize)]
struct TestBinaryRecord<'a> {
    id: &'a str,
    executable: &'a str,
    digest: &'a str,
}

#[derive(Serialize)]
struct ReverseLineIndexManifestMeta<'a> {
    schema_version: &'a str,
    snapshot_id: &'a str,
    meta_digest: &'a str,
    entry_state_revision: u64,
}

#[derive(Serialize)]
struct PopulationManifest<'a> {
    schema_version: &'a str,
    cache_schema_version: &'a str,
    source_root: String,
    generation_fingerprint: &'a str,
    input_fingerprint: &'a str,
    selection_context_fingerprint: &'a str,
    entries_fingerprint: &'a str,
    selectors: &'a [String],
    ordinary_source_digests: Vec<OrdinarySourceDigestRecord<'a>>,
    test_binaries: Vec<TestBinaryRecord<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reverse_line_index: Option<ReverseLineIndexManifestMeta<'a>>,
}

fn ordinary_source_digest_records(
    identity: &RustCoverageBatchIdentity,
) -> Vec<OrdinarySourceDigestRecord<'_>> {
    identity
        .ordinary_source_digests
        .iter()
        .map(|(path, digest)| OrdinarySourceDigestRecord {
            path: path.as_str(),
            digest: digest.as_str(),
        })
        .collect()
}

fn test_binary_records(test_binaries: &[RustTestBinaryIdentity]) -> Vec<TestBinaryRecord<'_>> {
    let mut sorted_binaries = test_binaries.iter().collect::<Vec<_>>();
    sorted_binaries.sort_by(|left, right| left.id.cmp(&right.id));
    sorted_binaries.dedup_by(|left, right| left.id == right.id);
    sorted_binaries
        .into_iter()
        .map(|binary| TestBinaryRecord {
            id: binary.id.as_str(),
            executable: binary.executable.as_str(),
            digest: binary.digest.as_str(),
        })
        .collect()
}
