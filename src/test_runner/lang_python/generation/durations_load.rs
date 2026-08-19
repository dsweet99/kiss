
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;

use super::publish::{
    GENERATION_DURATIONS_SCHEMA, GENERATION_DURATIONS_SCHEMA_V1, GenerationDurationsFile,
    PathMaxDuration, path_maxes_from_selector_durations,
};
use crate::test_runner::python_coverage_index::storage::python_coverage_cache_root;
use super::paths::{generation_dir, pointer_path};
use super::types::{POINTER_SCHEMA_VERSION, PopulationPointer};

static DURATIONS_MEMO: Mutex<Option<DurationsMemoEntry>> = Mutex::new(None);
static PATH_MAXES_MEMO: Mutex<Option<PathMaxesMemoEntry>> = Mutex::new(None);

struct DurationsMemoEntry {
    generation_id: String,
    pairs: Vec<(String, Duration)>,
    max: Duration,
    path_maxes: Vec<PathMaxDuration>,
}

struct PathMaxesMemoEntry {
    generation_id: String,
    path_maxes: Vec<PathMaxDuration>,
}

pub(crate) fn clear_generation_durations_memo() {
    if let Ok(mut guard) = DURATIONS_MEMO.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = PATH_MAXES_MEMO.lock() {
        *guard = None;
    }
}

pub(crate) fn try_load_generation_durations_pairs(
    repo_root: &Path,
) -> Option<Vec<(String, Duration)>> {
    Some(load_memoized(repo_root)?.pairs)
}

pub(crate) fn try_load_generation_max_duration(repo_root: &Path) -> Option<Duration> {
    Some(load_memoized(repo_root)?.max)
}

pub(crate) fn try_load_generation_path_maxes(
    repo_root: &Path,
) -> Option<Vec<PathMaxDuration>> {
    let path_maxes = load_memoized(repo_root)?.path_maxes;
    if path_maxes.is_empty() {
        return None;
    }
    Some(path_maxes)
}

struct LoadedDurations {
    pairs: Vec<(String, Duration)>,
    max: Duration,
    path_maxes: Vec<PathMaxDuration>,
}

fn load_memoized(repo_root: &Path) -> Option<LoadedDurations> {
    let cache_root = python_coverage_cache_root(repo_root).ok()?;
    let pointer = read_pointer(&cache_root)?;
    if let Ok(guard) = DURATIONS_MEMO.lock()
        && let Some(entry) = guard.as_ref()
        && entry.generation_id == pointer.generation_id
    {
        return Some(LoadedDurations {
            pairs: entry.pairs.clone(),
            max: entry.max,
            path_maxes: entry.path_maxes.clone(),
        });
    }
    let gen_dir = generation_dir(&cache_root, &pointer.generation_id);
    let mut file = read_durations_file(&gen_dir)?;
    let selectors = read_plan_selectors(&gen_dir)?;
    if file.durations_ns.len() != selectors.len() {
        return None;
    }
    let max = Duration::from_nanos(file.max_duration_ns);

    let pairs: Vec<(String, Duration)> = selectors
        .into_iter()
        .zip(file.durations_ns.iter().copied())
        .filter_map(|(selector, ns)| ns.map(|n| (selector, Duration::from_nanos(n))))
        .collect();
    if file.path_maxes.is_empty() {
        file.path_maxes = path_maxes_from_selector_durations(&pairs);
        let _ = persist_path_maxes(&gen_dir, &file);
    }
    let path_maxes = file.path_maxes;
    if let Ok(mut guard) = DURATIONS_MEMO.lock() {
        *guard = Some(DurationsMemoEntry {
            generation_id: pointer.generation_id.clone(),
            pairs: pairs.clone(),
            max,
            path_maxes: path_maxes.clone(),
        });
    }
    if let Ok(mut guard) = PATH_MAXES_MEMO.lock() {
        *guard = Some(PathMaxesMemoEntry {
            generation_id: pointer.generation_id,
            path_maxes: path_maxes.clone(),
        });
    }
    Some(LoadedDurations {
        pairs,
        max,
        path_maxes,
    })
}

pub(crate) fn try_load_generation_path_maxes_only(
    repo_root: &Path,
) -> Option<Vec<PathMaxDuration>> {
    let cache_root = python_coverage_cache_root(repo_root).ok()?;
    let pointer = read_pointer(&cache_root)?;
    if let Ok(guard) = DURATIONS_MEMO.lock()
        && let Some(entry) = guard.as_ref()
        && entry.generation_id == pointer.generation_id
        && !entry.path_maxes.is_empty()
    {
        return Some(entry.path_maxes.clone());
    }
    if let Ok(guard) = PATH_MAXES_MEMO.lock()
        && let Some(entry) = guard.as_ref()
        && entry.generation_id == pointer.generation_id
    {
        return Some(entry.path_maxes.clone());
    }
    let gen_dir = generation_dir(&cache_root, &pointer.generation_id);
    let file = read_durations_file(&gen_dir)?;
    if !file.path_maxes.is_empty() {
        if let Ok(mut guard) = PATH_MAXES_MEMO.lock() {
            *guard = Some(PathMaxesMemoEntry {
                generation_id: pointer.generation_id,
                path_maxes: file.path_maxes.clone(),
            });
        }
        return Some(file.path_maxes);
    }

    try_load_generation_path_maxes(repo_root)
}

fn persist_path_maxes(gen_dir: &Path, file: &GenerationDurationsFile) -> Result<(), ()> {
    let path = gen_dir.join("durations.json");
    let bytes = serde_json::to_vec_pretty(file).map_err(|_| ())?;
    let tmp = tmp_path_beside(&path);
    fs::write(&tmp, &bytes).map_err(|_| ())?;
    fs::rename(&tmp, &path).map_err(|_| ())?;
    Ok(())
}

fn tmp_path_beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("durations.json");
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

fn read_pointer(cache_root: &Path) -> Option<PopulationPointer> {
    let bytes = fs::read(pointer_path(cache_root)).ok()?;
    let pointer: PopulationPointer = serde_json::from_slice(&bytes).ok()?;
    (pointer.schema_version == POINTER_SCHEMA_VERSION).then_some(pointer)
}

fn read_durations_file(gen_dir: &Path) -> Option<GenerationDurationsFile> {
    let bytes = fs::read(gen_dir.join("durations.json")).ok()?;
    let file: GenerationDurationsFile = serde_json::from_slice(&bytes).ok()?;

    (file.schema_version == GENERATION_DURATIONS_SCHEMA
        || file.schema_version == GENERATION_DURATIONS_SCHEMA_V1)
        .then_some(file)
}

#[derive(Deserialize)]
struct ManifestSelectorsOnly {
    plan: PlanSelectorsOnly,
}

#[derive(Deserialize)]
struct PlanSelectorsOnly {
    selectors: Vec<String>,
}

fn read_plan_selectors(gen_dir: &Path) -> Option<Vec<String>> {
    let bytes = fs::read(gen_dir.join("manifest.json")).ok()?;
    let manifest: ManifestSelectorsOnly = serde_json::from_slice(&bytes).ok()?;
    Some(manifest.plan.selectors)
}

#[cfg(test)]
#[path = "durations_load_test.rs"]
mod tests;

