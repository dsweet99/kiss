use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use serde::{Deserialize, Serialize};

use super::POPULATION_SCHEMA_VERSION;
use super::manifest::PythonPopulationManifest;
use super::manifest::{
    PYTHON_COVERAGE_ENV_KEYS, read_python_population_manifest, stored_python_universe_population,
};
use super::storage::{python_coverage_cache_root, python_unique_suffix};
use crate::test_runner::runners::{detect_rslip_versions, rslip_request_from_parts};

pub(crate) const POPULATION_DURATIONS_SCHEMA: &str = "rslip-python-population-durations-v3";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PopulationDurationsFile {
    schema_version: String,
    cache_schema_version: String,
    input_fingerprint: String,
    entries_fingerprint: String,
    durations_ns: Vec<u64>,
    max_duration_ns: u64,
}

fn population_durations_path(cache_root: &Path) -> PathBuf {
    cache_root.join("population_durations.json")
}

pub(crate) fn load_current_python_population_durations(
    repo_root: &Path,
    pytest_args: &[String],
) -> Option<Vec<(String, Duration)>> {
    let population =
        stored_python_universe_population(repo_root, pytest_args, PYTHON_COVERAGE_ENV_KEYS)?;
    let manifest = read_python_population_manifest(repo_root)?;
    let cache_root = python_coverage_cache_root(repo_root).ok()?;
    if let Some(cached) = try_load_population_durations(&cache_root, &manifest) {
        return Some(cached);
    }
    let pairs = load_durations_from_entry_probes(repo_root, pytest_args, &population.selectors)?;
    let _ = write_population_durations(&cache_root, &manifest, &pairs);
    Some(pairs)
}

#[allow(dead_code)]
pub(crate) fn try_publish_python_population_durations(
    repo_root: &Path,
    pytest_args: &[String],
) -> Result<(), String> {
    let population =
        stored_python_universe_population(repo_root, pytest_args, PYTHON_COVERAGE_ENV_KEYS)
            .ok_or_else(|| {
                "error: kiss: Python population is missing after publication".to_string()
            })?;
    let manifest = read_python_population_manifest(repo_root).ok_or_else(|| {
        "error: kiss: Python population manifest missing after publication".to_string()
    })?;
    let cache_root = python_coverage_cache_root(repo_root)?;
    let pairs = load_durations_from_entry_probes(repo_root, pytest_args, &population.selectors)
        .ok_or_else(|| {
            "error: kiss: incomplete Python durations while publishing population sidecar"
                .to_string()
        })?;
    write_population_durations(&cache_root, &manifest, &pairs)
}

pub(crate) fn try_load_population_durations(
    cache_root: &Path,
    manifest: &PythonPopulationManifest,
) -> Option<Vec<(String, Duration)>> {
    let bytes = fs::read(population_durations_path(cache_root)).ok()?;
    let file: PopulationDurationsFile = serde_json::from_slice(&bytes).ok()?;
    if file.schema_version != POPULATION_DURATIONS_SCHEMA
        || file.cache_schema_version != manifest.cache_schema_version
        || file.input_fingerprint != manifest.input_fingerprint
        || file.entries_fingerprint != manifest.entries_fingerprint
        || manifest.schema_version != POPULATION_SCHEMA_VERSION
        || file.durations_ns.len() != manifest.selectors.len()
    {
        return None;
    }
    Some(
        manifest
            .selectors
            .iter()
            .zip(file.durations_ns.iter())
            .map(|(selector, nanos)| (selector.clone(), Duration::from_nanos(*nanos)))
            .collect(),
    )
}

pub(crate) fn write_population_durations(
    cache_root: &Path,
    manifest: &PythonPopulationManifest,
    pairs: &[(String, Duration)],
) -> Result<(), String> {
    if pairs.len() != manifest.selectors.len() {
        return Err("error: kiss: Python duration sidecar selector count mismatch".to_string());
    }
    let mut durations_ns = Vec::with_capacity(pairs.len());
    let mut max_duration_ns = 0_u64;
    for (expected, (selector, duration)) in manifest.selectors.iter().zip(pairs.iter()) {
        if expected != selector {
            return Err("error: kiss: Python duration sidecar selector order mismatch".to_string());
        }
        let nanos = duration.as_nanos() as u64;
        max_duration_ns = max_duration_ns.max(nanos);
        durations_ns.push(nanos);
    }
    let payload = PopulationDurationsFile {
        schema_version: POPULATION_DURATIONS_SCHEMA.to_string(),
        cache_schema_version: manifest.cache_schema_version.clone(),
        input_fingerprint: manifest.input_fingerprint.clone(),
        entries_fingerprint: manifest.entries_fingerprint.clone(),
        durations_ns,
        max_duration_ns,
    };
    let path = population_durations_path(cache_root);
    let parent = path
        .parent()
        .ok_or_else(|| "error: kiss: Python duration sidecar path has no parent".to_string())?;
    let tmp_path = parent.join(format!(
        ".population_durations.{}.tmp",
        python_unique_suffix()
    ));
    kiss::kiss_publication_barrier::publish_atomically(
        "python_population_durations",
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

fn load_durations_from_entry_probes(
    repo_root: &Path,
    pytest_args: &[String],
    selectors: &[String],
) -> Option<Vec<(String, Duration)>> {
    if let Some(out) = load_durations_from_fingerprinted_probes(repo_root, pytest_args, selectors) {
        return Some(out);
    }
    load_durations_from_scanned_probes(repo_root, selectors)
}

fn load_durations_from_fingerprinted_probes(
    repo_root: &Path,
    pytest_args: &[String],
    selectors: &[String],
) -> Option<Vec<(String, Duration)>> {
    let (python_version, pytest_version) = detect_rslip_versions(repo_root).ok()?;
    let mut out = Vec::with_capacity(selectors.len());
    for selector in selectors {
        let req = rslip_request_from_parts(
            repo_root,
            selector,
            pytest_args,
            &python_version,
            &pytest_version,
            false,
            &kiss::GateConfig::load_for_repo(repo_root),
        )
        .ok()?;
        let record = kiss::rslip::cached_record_for_request(&req).ok()??;
        if record.status != TestStatus::Passed {
            return None;
        }
        out.push((selector.clone(), record.duration));
    }
    Some(out)
}

fn load_durations_from_scanned_probes(
    repo_root: &Path,
    selectors: &[String],
) -> Option<Vec<(String, Duration)>> {
    let wanted: std::collections::BTreeSet<_> = selectors.iter().cloned().collect();
    let mut found = std::collections::BTreeMap::<String, Duration>::new();
    for path in super::storage::python_coverage_entry_paths(repo_root) {
        let Some(record) = kiss::test_records::read_record(&path) else {
            continue;
        };
        if !wanted.contains(&record.test_id) || record.status != TestStatus::Passed {
            continue;
        }
        found.insert(record.test_id, record.duration);
    }
    if selectors
        .iter()
        .any(|selector| !found.contains_key(selector))
    {
        return None;
    }
    Some(
        selectors
            .iter()
            .map(|selector| (selector.clone(), found[selector]))
            .collect(),
    )
}

#[cfg(test)]
#[path = "population_durations_test.rs"]
mod population_durations_test;
