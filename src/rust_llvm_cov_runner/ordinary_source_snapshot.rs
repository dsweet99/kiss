use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::rust_llvm_cov_runner::plan::batch_fingerprint::RustCoverageBatchIdentity;
use crate::rust_llvm_cov_runner::query_reverse_covering_files;

const SNAPSHOT_SCHEMA: &str = "kiss-ordinary-source-snapshot-v1";
const SNAPSHOT_FILE: &str = "ordinary_source_snapshot.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrdinarySourceInvalidation {
    None,
    Selectors(BTreeSet<String>),
    All,
}

impl OrdinarySourceInvalidation {
    pub fn invalidates(&self, selector: &str) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Selectors(set) => set.contains(selector),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct OrdinarySourceSnapshotFile {
    schema_version: String,
    generation_fingerprint: String,
    ordinary_source_digests: BTreeMap<String, String>,
    #[serde(default)]
    line_hashes: BTreeMap<String, Vec<u64>>,
}

pub fn write_ordinary_source_snapshot(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
) -> io::Result<()> {
    fs::create_dir_all(cache_root)?;
    let body = OrdinarySourceSnapshotFile {
        schema_version: SNAPSHOT_SCHEMA.to_string(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        ordinary_source_digests: identity.ordinary_source_digests.clone(),
        line_hashes: line_hashes_for_identity(source_root, identity),
    };
    let path = cache_root.join(SNAPSHOT_FILE);
    let tmp = cache_root.join(format!("{SNAPSHOT_FILE}.tmp-{}", std::process::id()));
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(&body).map_err(io::Error::other)?,
    )?;
    fs::rename(tmp, path)
}

pub fn load_ordinary_source_snapshot(
    cache_root: &Path,
    generation_fingerprint: &str,
) -> Option<BTreeMap<String, String>> {
    load_ordinary_source_snapshot_file(cache_root, generation_fingerprint)
        .map(|parsed| parsed.ordinary_source_digests)
}

pub fn load_ordinary_source_line_hashes(cache_root: &Path) -> Option<BTreeMap<String, Vec<u64>>> {
    let bytes = fs::read(cache_root.join(SNAPSHOT_FILE)).ok()?;
    let parsed: OrdinarySourceSnapshotFile = serde_json::from_slice(&bytes).ok()?;
    (parsed.schema_version == SNAPSHOT_SCHEMA).then_some(parsed.line_hashes)
}

pub fn remap_covered_file_lines(
    source_root: &Path,
    rel: &str,
    stored_hashes: &[u64],
    old_lines: &BTreeSet<u32>,
) -> BTreeSet<u32> {
    let Ok(bytes) = fs::read(source_root.join(rel)) else {
        return BTreeSet::new();
    };
    super::source_diff::remap_line_set(stored_hashes, &line_content_hashes(&bytes), old_lines)
}

fn load_ordinary_source_snapshot_file(
    cache_root: &Path,
    generation_fingerprint: &str,
) -> Option<OrdinarySourceSnapshotFile> {
    let bytes = fs::read(cache_root.join(SNAPSHOT_FILE)).ok()?;
    let parsed: OrdinarySourceSnapshotFile = serde_json::from_slice(&bytes).ok()?;
    (parsed.schema_version == SNAPSHOT_SCHEMA
        && parsed.generation_fingerprint == generation_fingerprint)
        .then_some(parsed)
}

pub fn classify_ordinary_source_delta(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
) -> OrdinarySourceInvalidation {
    let stored = stored_ordinary_source_digests_from_manifest(cache_root, identity)
        .or_else(|| {
            crate::rust_llvm_cov_runner::load_current_population_state(
                cache_root,
                source_root,
                identity,
                None,
            )
            .map(|state| state.ordinary_source_digests)
        })
        .or_else(|| load_ordinary_source_snapshot(cache_root, &identity.generation_fingerprint));
    let Some(stored) = stored else {
        return OrdinarySourceInvalidation::All;
    };
    if stored == identity.ordinary_source_digests {
        return OrdinarySourceInvalidation::None;
    }
    if stored.keys().ne(identity.ordinary_source_digests.keys()) {
        return OrdinarySourceInvalidation::All;
    }
    let modified: Vec<String> = stored
        .iter()
        .filter(|(path, digest)| identity.ordinary_source_digests.get(*path) != Some(*digest))
        .map(|(path, _)| path.clone())
        .collect();
    if let Some(selectors) = selectors_from_line_diff(cache_root, source_root, identity, &modified)
    {
        return if selectors.is_empty() {
            OrdinarySourceInvalidation::None
        } else {
            OrdinarySourceInvalidation::Selectors(selectors)
        };
    }
    match query_reverse_covering_files(cache_root, &identity.generation_fingerprint, &modified) {
        Some(selectors) if selectors.is_empty() => OrdinarySourceInvalidation::All,
        Some(selectors) => OrdinarySourceInvalidation::Selectors(selectors),
        None => OrdinarySourceInvalidation::All,
    }
}

fn selectors_from_line_diff(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    modified: &[String],
) -> Option<BTreeSet<String>> {
    let snapshot =
        load_ordinary_source_snapshot_file(cache_root, &identity.generation_fingerprint)?;
    let mut changed_rels: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for path in modified {
        let stored_hashes = snapshot.line_hashes.get(path)?;
        let bytes = fs::read(source_root.join(path)).ok()?;
        let diff = super::source_diff::diff_eq_slices(stored_hashes, &line_content_hashes(&bytes));
        if diff.ambiguous {
            return None;
        }
        if !diff.invalidated_old_lines.is_empty() {
            changed_rels.insert(path.clone(), diff.invalidated_old_lines);
        }
    }
    if changed_rels.is_empty() {
        return None;
    }
    let by_file = crate::rust_llvm_cov_runner::query_reverse_line_index(
        cache_root,
        &identity.generation_fingerprint,
        &changed_rels,
    )?;
    let mut selectors = BTreeSet::new();
    for file_selectors in by_file.values() {
        selectors.extend(file_selectors.iter().cloned());
    }
    if selectors.is_empty() {
        return None;
    }
    Some(selectors)
}

fn line_hashes_for_identity(
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
) -> BTreeMap<String, Vec<u64>> {
    let mut hashes = BTreeMap::new();
    for path in identity.ordinary_source_digests.keys() {
        if let Ok(bytes) = fs::read(source_root.join(path)) {
            hashes.insert(path.clone(), line_content_hashes(&bytes));
        }
    }
    hashes
}

fn line_content_hashes(bytes: &[u8]) -> Vec<u64> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|line| {
            crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(
                0xcbf2_9ce4_8422_2325,
                line.as_bytes(),
            )
        })
        .collect()
}

fn stored_ordinary_source_digests_from_manifest(
    cache_root: &Path,
    identity: &RustCoverageBatchIdentity,
) -> Option<BTreeMap<String, String>> {
    let manifest = crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::read_population_manifest(
        cache_root,
    )?;
    (manifest.generation_fingerprint == identity.generation_fingerprint)
        .then_some(manifest.ordinary_source_digests)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_rejects_other_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let identity = RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "gen-a".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: BTreeMap::from([("src/lib.rs".into(), "d".into())]),
        };
        write_ordinary_source_snapshot(tmp.path(), tmp.path(), &identity).unwrap();
        assert_eq!(
            load_ordinary_source_snapshot(tmp.path(), "gen-a")
                .unwrap()
                .get("src/lib.rs"),
            Some(&"d".to_string())
        );
        assert!(load_ordinary_source_snapshot(tmp.path(), "gen-b").is_none());
        assert!(!OrdinarySourceInvalidation::None.invalidates("a"));
        assert!(OrdinarySourceInvalidation::All.invalidates("a"));
        let only_a = OrdinarySourceInvalidation::Selectors(BTreeSet::from(["a".into()]));
        assert!(only_a.invalidates("a"));
        assert!(!only_a.invalidates("b"));
        assert_eq!(
            classify_ordinary_source_delta(tmp.path(), tmp.path(), &identity),
            OrdinarySourceInvalidation::None
        );
        assert!(
            remap_covered_file_lines(tmp.path(), "missing.rs", &[], &BTreeSet::from([1]))
                .is_empty()
        );
    }

    #[test]
    fn missing_stored_snapshot_fails_closed_to_all() {
        let tmp = tempfile::tempdir().unwrap();
        let identity = RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "gen-a".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: BTreeMap::from([("src/lib.rs".into(), "d".into())]),
        };
        assert_eq!(
            classify_ordinary_source_delta(tmp.path(), tmp.path(), &identity),
            OrdinarySourceInvalidation::All
        );
    }

    #[test]
    fn digest_mismatch_without_reverse_index_fails_closed_to_all() {
        let tmp = tempfile::tempdir().unwrap();
        let stored = RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "gen-a".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: BTreeMap::from([("src/lib.rs".into(), "old".into())]),
        };
        write_ordinary_source_snapshot(tmp.path(), tmp.path(), &stored).unwrap();
        let current = RustCoverageBatchIdentity {
            ordinary_source_digests: BTreeMap::from([("src/lib.rs".into(), "new".into())]),
            ..stored
        };
        assert_eq!(
            classify_ordinary_source_delta(tmp.path(), tmp.path(), &current),
            OrdinarySourceInvalidation::All
        );
    }

    #[test]
    fn insert_only_edit_without_reverse_index_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "a\nb\n").unwrap();
        let stored = RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "gen-a".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: BTreeMap::from([("src/lib.rs".into(), "old".into())]),
        };
        write_ordinary_source_snapshot(tmp.path(), tmp.path(), &stored).unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "a\nX\nb\n").unwrap();
        let current = RustCoverageBatchIdentity {
            ordinary_source_digests: BTreeMap::from([("src/lib.rs".into(), "new".into())]),
            ..stored
        };
        assert_eq!(
            classify_ordinary_source_delta(tmp.path(), tmp.path(), &current),
            OrdinarySourceInvalidation::All
        );
    }
}
