use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::POPULATION_SCHEMA_VERSION;
use super::manifest::read_python_population_manifest;
use super::storage::{python_coverage_cache_root, python_unique_suffix};

pub(crate) const COVERAGE_SNAPSHOT_SCHEMA: &str = "rslip-python-coverage-snapshot-v2";

pub(crate) type CoveredLinesMap = BTreeMap<String, BTreeSet<u32>>;

#[derive(Serialize, Deserialize)]
struct CoverageSnapshotFile {
    schema_version: String,
    cache_schema_version: String,
    input_fingerprint: String,
    entries_fingerprint: String,
    test_definition_digests: BTreeMap<String, String>,
    covered_lines: CoveredLinesMap,
}

fn coverage_snapshot_path(cache_root: &Path) -> PathBuf {
    cache_root.join("coverage_snapshot.json")
}

pub(crate) fn try_load_python_coverage_snapshot(repo_root: &Path) -> Option<CoveredLinesMap> {
    let manifest = read_python_population_manifest(repo_root)?;
    if manifest.schema_version != POPULATION_SCHEMA_VERSION {
        return None;
    }
    let cache_root = python_coverage_cache_root(repo_root).ok()?;
    let bytes = fs::read(coverage_snapshot_path(&cache_root)).ok()?;
    let file: CoverageSnapshotFile = serde_json::from_slice(&bytes).ok()?;
    if file.schema_version != COVERAGE_SNAPSHOT_SCHEMA
        || file.cache_schema_version != manifest.cache_schema_version
        || file.input_fingerprint != manifest.input_fingerprint
        || file.entries_fingerprint != manifest.entries_fingerprint
        || file.test_definition_digests != test_definition_digests(repo_root, &manifest.selectors)
    {
        return None;
    }
    Some(file.covered_lines)
}

pub(crate) fn write_python_coverage_snapshot(
    repo_root: &Path,
    covered_lines: &CoveredLinesMap,
) -> Result<(), String> {
    let Some(manifest) = read_python_population_manifest(repo_root) else {
        return Ok(());
    };
    let cache_root = python_coverage_cache_root(repo_root)?;
    let test_definition_digests = test_definition_digests(repo_root, &manifest.selectors);
    let payload = CoverageSnapshotFile {
        schema_version: COVERAGE_SNAPSHOT_SCHEMA.to_string(),
        cache_schema_version: manifest.cache_schema_version,
        input_fingerprint: manifest.input_fingerprint,
        entries_fingerprint: manifest.entries_fingerprint,
        test_definition_digests,
        covered_lines: covered_lines.clone(),
    };
    let path = coverage_snapshot_path(&cache_root);
    let parent = path
        .parent()
        .ok_or_else(|| "error: kiss: Python coverage snapshot path has no parent".to_string())?;
    let tmp_path = parent.join(format!(".coverage_snapshot.{}.tmp", python_unique_suffix()));
    kiss::kiss_publication_barrier::publish_atomically(
        "python_coverage_snapshot",
        &path,
        &tmp_path,
        |file| {
            serde_json::to_writer(&mut *file, &payload).map_err(std::io::Error::other)?;
            use std::io::Write;
            file.write_all(b"\n")?;
            Ok(())
        },
    )
    .map_err(|e| e.to_string())
}

fn test_definition_digests(repo_root: &Path, selectors: &[String]) -> BTreeMap<String, String> {
    selectors
        .iter()
        .map(|selector| {
            (
                selector.clone(),
                super::storage::python_selector_definition_digest(repo_root, selector),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_misses_after_test_definition_change() {
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join("app.py"), "x = 1\n").unwrap();
        std::fs::write(repo.join("test_app.py"), "def test_a(): pass\n").unwrap();
        let selector = "test_app.py::test_a".to_string();
        if super::super::manifest::write_python_population_manifest_for_args(
            repo,
            std::slice::from_ref(&selector),
            &[],
        )
        .is_err()
        {
            return;
        }
        write_python_coverage_snapshot(repo, &BTreeMap::new()).unwrap();
        assert!(try_load_python_coverage_snapshot(repo).is_some());
        std::fs::write(repo.join("test_app.py"), "def test_a(): pass  # changed\n").unwrap();
        assert!(try_load_python_coverage_snapshot(repo).is_none());
    }
}
