
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rpytest_runner::TestStatus;
use serde::{Deserialize, Serialize};

use super::manifest::{
    PYTHON_COVERAGE_ENV_KEYS, read_python_population_manifest, stored_python_universe_population,
};
use super::manifest::PythonPopulationManifest;
use super::storage::{python_coverage_cache_root, python_unique_suffix};
use super::POPULATION_SCHEMA_VERSION;
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
    if let Some(pairs) = super::generation::try_load_generation_durations_pairs(repo_root) {
        return Some(pairs);
    }
    if let Ok(pinned) = super::generation::try_load_pinned_python_generation_warm(repo_root) {
        let exec =
            super::generation::current_python_execution_identity(repo_root, pytest_args).ok()?;
        if pinned.plan.base_identity == exec {
            return Some(
                pinned
                    .timings
                    .into_iter()
                    .filter_map(|row| {
                        row.duration_ns
                            .map(|ns| (row.selector, Duration::from_nanos(ns)))
                    })
                    .collect(),
            );
        }
        return None;
    }
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

pub(crate) fn load_current_python_population_max_duration(
    repo_root: &Path,
    pytest_args: &[String],
) -> Option<Duration> {
    if let Some(max) = super::generation::try_load_generation_max_duration(repo_root) {
        return Some(max);
    }
    if let Ok(pinned) = super::generation::try_load_pinned_python_generation_warm(repo_root) {
        let exec =
            super::generation::current_python_execution_identity(repo_root, pytest_args).ok()?;
        if pinned.plan.base_identity == exec {
            let max_ns = pinned
                .timings
                .iter()
                .filter_map(|row| row.duration_ns)
                .max()
                .unwrap_or(0);
            return Some(Duration::from_nanos(max_ns));
        }
        return None;
    }
    let identity = super::manifest::current_python_population_manifest_identity(repo_root, pytest_args)
        .ok()?;
    let cache_root = python_coverage_cache_root(repo_root).ok()?;
    let pop_path = cache_root.join("population.json");
    let pop_bytes = fs::read(&pop_path).ok()?;
    let pop_id: PopulationIdentityOnly = serde_json::from_slice(&pop_bytes).ok()?;
    if pop_id.schema_version != POPULATION_SCHEMA_VERSION
        || pop_id.cache_schema_version != identity.cache_schema_version
        || pop_id.python_version != identity.python_version
        || pop_id.pytest_version != identity.pytest_version
        || pop_id.pytest_args != identity.pytest_args
        || pop_id.env != identity.env
    {
        return None;
    }
    let dur_path = population_durations_path(&cache_root);
    let bytes = match fs::read(&dur_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            let _ = load_current_python_population_durations(repo_root, pytest_args)?;
            fs::read(&dur_path).ok()?
        }
    };
    let file: PopulationDurationsMaxOnly = serde_json::from_slice(&bytes).ok()?;
    if file.schema_version != POPULATION_DURATIONS_SCHEMA
        || file.cache_schema_version != pop_id.cache_schema_version
        || file.input_fingerprint != pop_id.input_fingerprint
        || file.entries_fingerprint != pop_id.entries_fingerprint
    {

        let _ = load_current_python_population_durations(repo_root, pytest_args)?;
        let bytes = fs::read(&dur_path).ok()?;
        let file: PopulationDurationsMaxOnly = serde_json::from_slice(&bytes).ok()?;
        return Some(Duration::from_nanos(file.max_duration_ns));
    }
    Some(Duration::from_nanos(file.max_duration_ns))
}

pub(crate) fn load_current_python_population_path_maxes(
    repo_root: &Path,
    pytest_args: &[String],
) -> Option<Vec<super::generation::PathMaxDuration>> {
    if let Some(path_maxes) = super::generation::try_load_generation_path_maxes_only(repo_root) {
        return Some(path_maxes);
    }
    let pairs = load_current_python_population_durations(repo_root, pytest_args)?;
    Some(super::generation::path_maxes_from_selector_durations(&pairs))
}

#[derive(Deserialize)]
struct PopulationDurationsMaxOnly {
    schema_version: String,
    cache_schema_version: String,
    input_fingerprint: String,
    entries_fingerprint: String,
    max_duration_ns: u64,
}

#[derive(Deserialize)]
struct PopulationIdentityOnly {
    schema_version: String,
    cache_schema_version: String,
    python_version: String,
    pytest_version: String,
    pytest_args: Vec<String>,
    env: std::collections::BTreeMap<String, String>,
    input_fingerprint: String,
    entries_fingerprint: String,
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
    kiss_publication_barrier::publish_atomically("python_population_durations", &path, &tmp_path, |file| {
        serde_json::to_writer(&mut *file, &payload).map_err(std::io::Error::other)?;
        use std::io::Write;
        file.write_all(b"\n")?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
struct DurationProbeEntry {
    nodeid: String,
    status: TestStatus,
    duration: Duration,
}

fn load_durations_from_entry_probes(
    repo_root: &Path,
    pytest_args: &[String],
    selectors: &[String],
) -> Option<Vec<(String, Duration)>> {
    load_durations_from_entry_probes_inner(repo_root, pytest_args, selectors, true)
}

pub(crate) fn load_durations_from_entry_probes_allow_non_passed(
    repo_root: &Path,
    pytest_args: &[String],
    selectors: &[String],
) -> Option<Vec<(String, Duration)>> {
    load_durations_from_entry_probes_inner(repo_root, pytest_args, selectors, false)
}

fn load_durations_from_entry_probes_inner(
    repo_root: &Path,
    pytest_args: &[String],
    selectors: &[String],
    require_passed: bool,
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
        let fingerprint = rslip::cache_fingerprint_for_request(&req).ok()?;
        let path = req
            .cache_root
            .join("entries")
            .join(format!("{fingerprint}.json"));
        let bytes = fs::read(path).ok()?;
        let entry: DurationProbeEntry = serde_json::from_slice(&bytes).ok()?;
        if entry.nodeid != *selector {
            return None;
        }
        if require_passed && entry.status != TestStatus::Passed {
            return None;
        }
        out.push((selector.clone(), entry.duration));
    }
    Some(out)
}

#[cfg(test)]
#[path = "population_durations_test.rs"]
mod population_durations_test;
