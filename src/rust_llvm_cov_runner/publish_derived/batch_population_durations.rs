use crate::rust_llvm_cov_runner::CACHE_SCHEMA_VERSION;
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity, entry_fingerprint,
};
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::RustPopulationState;
use crate::rust_llvm_cov_runner::rust_cov_cache::{
    load_rust_cov_cache_entry, rust_cov_unique_suffix,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) const POPULATION_DURATIONS_SCHEMA: &str = "rust-population-durations-v2";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PopulationDurationsFile {
    schema_version: String,
    cache_schema_version: String,
    generation_fingerprint: String,
    input_fingerprint: String,
    entries_fingerprint: String,
    entry_state_revision: u64,
    entries_stamp: Option<String>,
    durations: BTreeMap<String, u64>,
}

pub(crate) fn population_durations_path(cache_root: &Path) -> PathBuf {
    cache_root.join("population_durations.json")
}

#[cfg(test)]
pub(crate) fn invalidate_population_durations(cache_root: &Path) {
    let Ok(_guard) = population_durations_lock(cache_root) else {
        return;
    };
    let _ = fs::remove_file(population_durations_path(cache_root));
}

pub(crate) fn invalidate_population_durations_for_entry_write(cache_root: &Path) -> io::Result<()> {
    let _guard = population_durations_lock(cache_root)?;
    match fs::remove_file(population_durations_path(cache_root)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn population_durations_lock(
    cache_root: &Path,
) -> io::Result<crate::rust_llvm_cov_runner::file_lock::FileLockGuard> {
    crate::rust_llvm_cov_runner::file_lock::FileLockGuard::lock(
        &cache_root.join("locks").join("population_durations.lock"),
    )
}

pub fn population_entries_all_pass(cache_root: &Path, population: &RustPopulationState) -> bool {
    population_nonpassed_selectors(cache_root, population).is_empty()
}

pub fn population_nonpassed_selectors(
    cache_root: &Path,
    population: &RustPopulationState,
) -> BTreeSet<String> {
    if try_load_population_durations(cache_root, population).is_some() {
        return BTreeSet::new();
    }
    let expected: BTreeSet<&str> = population.selectors.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut nonpassed = BTreeSet::new();
    let Ok(entries) = fs::read_dir(cache_root.join("entries")) else {
        return population.selectors.iter().cloned().collect();
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(entry) = fs::read(path).ok().and_then(|bytes| {
            serde_json::from_slice::<crate::rust_llvm_cov_runner::RustCovCacheEntry>(&bytes).ok()
        }) else {
            continue;
        };
        if entry.generation_fingerprint != population.generation_fingerprint
            || !expected.contains(entry.selector.as_str())
        {
            continue;
        }
        if entry.schema_version != CACHE_SCHEMA_VERSION
            || entry.status != crate::rpytest_runner::TestStatus::Passed
        {
            nonpassed.insert(entry.selector.clone());
        }
        seen.insert(entry.selector);
    }
    nonpassed.extend(
        population
            .selectors
            .iter()
            .filter(|selector| !seen.contains(selector.as_str()))
            .cloned(),
    );
    nonpassed
}

pub fn load_current_population_durations(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    selectors: Option<&[String]>,
) -> Option<Vec<(String, Duration)>> {
    if let Some(cached) = try_load_durations_from_manifest_sidecar(cache_root, identity, selectors)
    {
        return Some(cached);
    }
    let population = crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::load_current_population_state(
        cache_root,
        source_root,
        identity,
        selectors,
    )?;
    let _guard = population_durations_lock(cache_root).ok()?;
    if let Some(cached) = try_load_population_durations(cache_root, &population) {
        return Some(cached);
    }
    let out = load_durations_from_entries(cache_root, &population, identity, req, tools)?;
    let _ = write_population_durations_under_lock(cache_root, &population, &out);
    Some(out)
}

fn try_load_durations_from_manifest_sidecar(
    cache_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: Option<&[String]>,
) -> Option<Vec<(String, Duration)>> {
    let manifest =
        crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::read_population_manifest(cache_root)?;
    if manifest.input_fingerprint != identity.input_digest
        || manifest.generation_fingerprint != identity.generation_fingerprint
        || manifest.selection_context_fingerprint != identity.selection_context_fingerprint
    {
        return None;
    }
    if let Some(selectors) = selectors {
        let mut expected = selectors.to_vec();
        expected.sort();
        expected.dedup();
        if manifest.selectors != expected {
            return None;
        }
    }
    let population = RustPopulationState {
        input_fingerprint: manifest.input_fingerprint,
        generation_fingerprint: manifest.generation_fingerprint,
        selection_context_fingerprint: manifest.selection_context_fingerprint,
        entries_fingerprint: manifest.entries_fingerprint,
        selectors: manifest.selectors,
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    population_entries_all_pass(cache_root, &population).then_some(())?;
    try_load_population_durations(cache_root, &population)
}

pub fn try_load_population_durations(
    cache_root: &Path,
    population: &RustPopulationState,
) -> Option<Vec<(String, Duration)>> {
    let before = current_entry_state(cache_root, population)?;
    let bytes = fs::read(population_durations_path(cache_root)).ok()?;
    let file: PopulationDurationsFile = serde_json::from_slice(&bytes).ok()?;
    if file.schema_version != POPULATION_DURATIONS_SCHEMA
        || file.cache_schema_version != CACHE_SCHEMA_VERSION
        || file.generation_fingerprint != population.generation_fingerprint
        || file.input_fingerprint != population.input_fingerprint
        || file.entries_fingerprint != population.entries_fingerprint
        || file.entry_state_revision != before.revision
        || file.entries_stamp != entries_stamp(cache_root)
    {
        return None;
    }
    let after = current_entry_state(cache_root, population)?;
    (after == before).then_some(())?;
    let mut out = Vec::with_capacity(population.selectors.len());
    for selector in &population.selectors {
        let nanos = *file.durations.get(selector)?;
        out.push((selector.clone(), Duration::from_nanos(nanos)));
    }
    Some(out)
}

#[cfg(test)]
pub(crate) fn write_population_durations(
    cache_root: &Path,
    population: &RustPopulationState,
    pairs: &[(String, Duration)],
) -> io::Result<()> {
    if crate::rust_llvm_cov_runner::publish_derived::batch_entry_state::read_entry_state(cache_root)
        .is_none()
    {
        crate::rust_llvm_cov_runner::publish_derived::batch_entry_state::publish_next_entry_state(
            cache_root,
            &population.generation_fingerprint,
            &population.entries_fingerprint,
        )
        .map_err(|err| io::Error::other(format!("{err:?}")))?;
    }
    let _guard = population_durations_lock(cache_root)?;
    write_population_durations_under_lock(cache_root, population, pairs)
}

fn write_population_durations_under_lock(
    cache_root: &Path,
    population: &RustPopulationState,
    pairs: &[(String, Duration)],
) -> io::Result<()> {
    let entry_state = current_entry_state(cache_root, population)
        .ok_or_else(|| io::Error::other("population durations missing current entry state"))?;
    let mut durations = BTreeMap::new();
    for (selector, duration) in pairs {
        durations.insert(selector.clone(), duration_as_nanos(*duration));
    }
    if durations.len() != population.selectors.len() {
        return Err(io::Error::other(
            "population durations selector count mismatch",
        ));
    }
    for selector in &population.selectors {
        if !durations.contains_key(selector) {
            return Err(io::Error::other("population durations missing selector"));
        }
    }
    let payload = PopulationDurationsFile {
        schema_version: POPULATION_DURATIONS_SCHEMA.to_string(),
        cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
        generation_fingerprint: population.generation_fingerprint.clone(),
        input_fingerprint: population.input_fingerprint.clone(),
        entries_fingerprint: population.entries_fingerprint.clone(),
        entry_state_revision: entry_state.revision,
        entries_stamp: entries_stamp(cache_root),
        durations,
    };
    let path = population_durations_path(cache_root);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("population_durations path has no parent"))?;
    let tmp = parent.join(format!(
        ".population_durations.{}.tmp",
        rust_cov_unique_suffix()
    ));
    crate::kiss_publication_barrier::publish_atomically(
        "rust_population_durations",
        &path,
        &tmp,
        |file| {
            serde_json::to_writer(&mut *file, &payload).map_err(io::Error::other)?;
            file.write_all(b"\n")?;
            Ok(())
        },
    )
}

fn entries_stamp(cache_root: &Path) -> Option<String> {
    let metadata = fs::metadata(cache_root.join("entries")).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(format!(
            "{}:{}:{}:{}:{}",
            metadata.len(),
            modified.as_nanos(),
            metadata.dev(),
            metadata.ino(),
            metadata.ctime_nsec()
        ))
    }
    #[cfg(not(unix))]
    {
        Some(format!("{}:{}", metadata.len(), modified.as_nanos()))
    }
}

fn current_entry_state(
    cache_root: &Path,
    population: &RustPopulationState,
) -> Option<crate::rust_llvm_cov_runner::EntryState> {
    let state = crate::rust_llvm_cov_runner::publish_derived::batch_entry_state::read_entry_state(
        cache_root,
    )?;
    (state.generation_fingerprint == population.generation_fingerprint
        && state.entries_fingerprint == population.entries_fingerprint)
        .then_some(state)
}

pub(crate) fn try_publish_durations_after_population(
    cache_root: &Path,
    identity: &RustCoverageBatchIdentity,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    selectors: &[String],
    entries_fingerprint: &str,
) {
    let population = RustPopulationState {
        input_fingerprint: identity.input_digest.clone(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        entries_fingerprint: entries_fingerprint.to_string(),
        selectors: selectors.to_vec(),
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    let Ok(_guard) = population_durations_lock(cache_root) else {
        return;
    };
    let Some(pairs) = load_durations_from_entries(cache_root, &population, identity, req, tools)
    else {
        return;
    };
    let _ = write_population_durations_under_lock(cache_root, &population, &pairs);
}

pub(crate) fn load_durations_from_entries(
    cache_root: &Path,
    population: &RustPopulationState,
    identity: &RustCoverageBatchIdentity,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> Option<Vec<(String, Duration)>> {
    let mut out = Vec::with_capacity(population.selectors.len());
    for selector in &population.selectors {
        let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, selector);
        let entry = load_rust_cov_cache_entry(cache_root, &fingerprint)?;
        if entry.selector != *selector || entry.status != crate::rpytest_runner::TestStatus::Passed
        {
            return None;
        }
        if entry.generation_fingerprint != population.generation_fingerprint {
            return None;
        }
        out.push((selector.clone(), entry.duration));
    }
    Some(out)
}

fn duration_as_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "batch_population_durations_test.rs"]
mod tests;
