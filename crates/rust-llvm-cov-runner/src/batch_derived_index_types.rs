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

#[derive(Deserialize)]
pub(crate) struct PopulationManifestRaw {
    pub(crate) schema_version: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) input_fingerprint: String,
    pub(crate) selection_context_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) selectors: Vec<String>,
    pub(crate) ordinary_source_digests: Vec<OrdinarySourceDigestRecord>,
}

pub(crate) struct PopulationManifestOnDisk {
    pub(crate) schema_version: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) input_fingerprint: String,
    pub(crate) selection_context_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) selectors: Vec<String>,
    pub(crate) ordinary_source_digests: BTreeMap<String, String>,
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
