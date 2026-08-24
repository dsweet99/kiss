use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::Deserialize;

pub(crate) type RustCoverageIndex = BTreeMap<String, std::collections::BTreeSet<String>>;

#[derive(Deserialize)]
pub(crate) struct OnDiskIndexWithFiles {
    pub(crate) schema_version: String,
    pub(crate) source_root: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) files: RustCoverageIndex,
}

#[cfg(test)]
#[derive(Deserialize)]
pub(crate) struct OnDiskIndex {
    pub(crate) schema_version: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) entries_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct OrdinarySourceDigestRecord {
    pub(crate) path: String,
    pub(crate) digest: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct TestBinaryRecord {
    pub(crate) id: String,
    pub(crate) executable: String,
    pub(crate) digest: String,
}

#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct ReverseLineIndexManifestMeta {
    pub(crate) schema_version: String,
    pub(crate) snapshot_id: String,
    pub(crate) meta_digest: String,
    pub(crate) entry_state_revision: u64,
}

#[derive(Deserialize)]
pub(crate) struct PopulationManifestRaw {
    pub(crate) schema_version: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) input_fingerprint: String,
    pub(crate) selection_context_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) selectors: Vec<String>,
    pub(crate) ordinary_source_digests: Vec<OrdinarySourceDigestRecord>,
    pub(crate) test_binaries: Vec<TestBinaryRecord>,
    #[serde(default)]
    pub(crate) reverse_line_index: Option<ReverseLineIndexManifestMeta>,
}

pub(crate) struct PopulationManifestOnDisk {
    pub(crate) schema_version: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) input_fingerprint: String,
    pub(crate) selection_context_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) selectors: Vec<String>,
    pub(crate) ordinary_source_digests: BTreeMap<String, String>,
    pub(crate) test_binaries: BTreeMap<String, crate::RustTestBinaryIdentity>,
    pub(crate) reverse_line_index: Option<ReverseLineIndexManifestMeta>,
}

pub(crate) fn validate_ordinary_source_digests(
    records: Vec<OrdinarySourceDigestRecord>,
) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let mut previous: Option<String> = None;
    for record in records {
        if previous.as_ref().is_some_and(|prev| record.path <= *prev) {
            return None;
        }
        if !valid_ordinary_source_path(&record.path)
            || !valid_ordinary_source_digest(&record.digest)
        {
            return None;
        }
        if out.insert(record.path.clone(), record.digest).is_some() {
            return None;
        }
        previous = Some(record.path);
    }
    Some(out)
}

pub(crate) fn validate_test_binaries(
    records: Vec<TestBinaryRecord>,
) -> Option<BTreeMap<String, crate::RustTestBinaryIdentity>> {
    let mut out = BTreeMap::new();
    let mut previous: Option<String> = None;
    for record in records {
        if previous.as_ref().is_some_and(|prev| record.id <= *prev) {
            return None;
        }
        if record.id.is_empty()
            || record.executable.is_empty()
            || !valid_ordinary_source_digest(&record.digest)
        {
            return None;
        }
        let item = crate::RustTestBinaryIdentity {
            id: record.id.clone(),
            executable: record.executable,
            digest: record.digest,
        };
        if out.insert(record.id.clone(), item).is_some() {
            return None;
        }
        previous = Some(record.id);
    }
    Some(out)
}

fn valid_ordinary_source_path(path: &str) -> bool {
    if path.is_empty() || !(path.ends_with(".rs") || path.ends_with(".inc")) || path.contains('\\')
    {
        return false;
    }
    let path = Path::new(path);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn valid_ordinary_source_digest(digest: &str) -> bool {
    digest.len() == 16
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
