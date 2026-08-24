use std::fs;
use std::path::Path;

use kiss_publication_barrier::publish_atomically;

use super::evidence::PopulationEvidence;
use super::paths::{
    create_staging_dir, generation_dir, generations_dir, pointer_path, sha256_hex, sync_dir,
    write_json_artifact,
};
use super::types::{
    ArtifactDigest, GENERATION_SCHEMA_VERSION, GenerationManifest, GenerationReason,
    POINTER_SCHEMA_VERSION, PopulationPointer, PythonPopulationPlan, SelectorTimingRecord,
};
use crate::test_runner::python_coverage_index::storage::{
    python_coverage_cache_root, python_unique_suffix,
};

pub(crate) fn publish_python_population_generation(
    repo_root: &Path,
    plan: &PythonPopulationPlan,
    evidence: &PopulationEvidence,
    reason: GenerationReason,
) -> Result<String, String> {
    publish_python_population_generation_reusing(repo_root, plan, evidence, reason, None)
}

pub(crate) fn publish_python_population_generation_reusing(
    repo_root: &Path,
    plan: &PythonPopulationPlan,
    evidence: &PopulationEvidence,
    reason: GenerationReason,
    reuse_generation_id: Option<&str>,
) -> Result<String, String> {
    let cache_root = python_coverage_cache_root(repo_root)?;
    let _guard = rslip::lock_rslip_derived_state(&cache_root).map_err(|e| e.to_string())?;
    let id = publish_locked(&cache_root, plan, evidence, reason, reuse_generation_id)?;
    super::memo::clear_python_generation_warm_memo();
    Ok(id)
}

pub(crate) fn publish_locked(
    cache_root: &Path,
    plan: &PythonPopulationPlan,
    evidence: &PopulationEvidence,
    reason: GenerationReason,
    reuse_generation_id: Option<&str>,
) -> Result<String, String> {
    let generation_id = format!("gen-{}", python_unique_suffix());
    let staged = create_staging_dir(cache_root)?;
    let mut artifacts = Vec::new();
    let reuse_dir = reuse_generation_id.map(|id| generation_dir(cache_root, id));
    if let Some(src) = reuse_dir.as_deref() {
        push_hardlinked_artifact(&mut artifacts, &staged, src, "coverage.json")?;
        push_hardlinked_artifact(&mut artifacts, &staged, src, "selector_coverage.json")?;
        push_artifact(
            &mut artifacts,
            &staged,
            "line_index.json",
            &evidence.line_index,
        )?;
        push_hardlinked_artifact(&mut artifacts, &staged, src, "timings.json")?;
        push_hardlinked_artifact(&mut artifacts, &staged, src, "durations.json")?;
    } else {
        push_artifact(&mut artifacts, &staged, "coverage.json", &evidence.coverage)?;
        push_artifact(
            &mut artifacts,
            &staged,
            "selector_coverage.json",
            &evidence.selector_coverage,
        )?;
        push_artifact(
            &mut artifacts,
            &staged,
            "line_index.json",
            &evidence.line_index,
        )?;
        push_artifact(&mut artifacts, &staged, "timings.json", &evidence.timings)?;
        let durations = generation_durations_file(&evidence.timings);
        push_artifact(&mut artifacts, &staged, "durations.json", &durations)?;
    }
    let manifest = GenerationManifest {
        schema_version: GENERATION_SCHEMA_VERSION.to_string(),
        generation_id: generation_id.clone(),
        plan: plan.clone(),
        complete: evidence.complete,
        artifacts,
        creation_reason: reason,
    };
    let (manifest_bytes, manifest_sha) = write_json_artifact(&staged, "manifest.json", &manifest)?;
    let _ = manifest_bytes;
    sync_dir(&staged)?;
    let final_dir = generation_dir(cache_root, &generation_id);
    fs::rename(&staged, &final_dir).map_err(|e| {
        format!(
            "error: kiss: rename generation staging {} -> {}: {e}",
            staged.display(),
            final_dir.display()
        )
    })?;
    sync_dir(&generations_dir(cache_root))?;
    write_pointer(cache_root, &generation_id, &manifest_sha)?;
    let _ = prune_old_generations(cache_root, &generation_id);
    Ok(generation_id)
}

fn push_artifact<T: serde::Serialize>(
    artifacts: &mut Vec<ArtifactDigest>,
    staged: &Path,
    name: &str,
    value: &T,
) -> Result<(), String> {
    let (bytes, digest) = write_json_artifact(staged, name, value)?;
    artifacts.push(ArtifactDigest {
        name: name.to_string(),
        byte_length: bytes.len() as u64,
        sha256: digest,
    });
    Ok(())
}

fn push_hardlinked_artifact(
    artifacts: &mut Vec<ArtifactDigest>,
    staged: &Path,
    src_dir: &Path,
    name: &str,
) -> Result<(), String> {
    super::paths::validate_artifact_name(name)?;
    let src = src_dir.join(name);
    let dst = staged.join(name);
    if fs::hard_link(&src, &dst).is_err() {
        fs::copy(&src, &dst).map_err(|e| {
            format!(
                "error: kiss: reuse generation artifact `{name}` {} -> {}: {e}",
                src.display(),
                dst.display()
            )
        })?;
        let file = fs::File::open(&dst)
            .map_err(|e| format!("error: kiss: open reused artifact {}: {e}", dst.display()))?;
        file.sync_all()
            .map_err(|e| format!("error: kiss: sync reused artifact {}: {e}", dst.display()))?;
    }
    let (byte_length, sha256) = super::paths::sha256_file(&dst)?;
    artifacts.push(ArtifactDigest {
        name: name.to_string(),
        byte_length,
        sha256,
    });
    Ok(())
}

fn write_pointer(
    cache_root: &Path,
    generation_id: &str,
    manifest_sha256: &str,
) -> Result<(), String> {
    let path = pointer_path(cache_root);
    let tmp = path.with_file_name(format!(".population.{}.tmp", python_unique_suffix()));
    let pointer = PopulationPointer {
        schema_version: POINTER_SCHEMA_VERSION.to_string(),
        generation_id: generation_id.to_string(),
        manifest_sha256: manifest_sha256.to_string(),
    };
    publish_atomically("python_population_pointer", &path, &tmp, |file| {
        use std::io::Write;
        let bytes = serde_json::to_vec_pretty(&pointer).map_err(std::io::Error::other)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

fn prune_old_generations(cache_root: &Path, keep_id: &str) -> Result<(), String> {
    let root = generations_dir(cache_root);
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') || name == keep_id {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let _ = fs::remove_dir_all(path);
        }
    }
    sync_dir(&root)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn pointer_digest_for_tests(generation_id: &str, manifest_sha: &str) -> String {
    sha256_hex(format!("{generation_id}:{manifest_sha}").as_bytes())
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct PathMaxDuration {
    pub(crate) path: String,
    pub(crate) max_duration_ns: u64,
    pub(crate) example_selector: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct GenerationDurationsFile {
    pub(crate) schema_version: String,
    pub(crate) durations_ns: Vec<Option<u64>>,
    pub(crate) max_duration_ns: u64,
    #[serde(default)]
    pub(crate) path_maxes: Vec<PathMaxDuration>,
}

pub(crate) const GENERATION_DURATIONS_SCHEMA: &str = "rslip-python-generation-durations-v2";
pub(crate) const GENERATION_DURATIONS_SCHEMA_V1: &str = "rslip-python-generation-durations-v1";

pub(crate) fn generation_durations_file(
    timings: &[SelectorTimingRecord],
) -> GenerationDurationsFile {
    let mut max_duration_ns = 0_u64;
    let mut durations_ns = Vec::with_capacity(timings.len());
    for row in timings {
        if let Some(ns) = row.duration_ns {
            max_duration_ns = max_duration_ns.max(ns);
        }
        durations_ns.push(row.duration_ns);
    }
    GenerationDurationsFile {
        schema_version: GENERATION_DURATIONS_SCHEMA.to_string(),
        durations_ns,
        max_duration_ns,
        path_maxes: path_maxes_from_timing_rows(timings),
    }
}

pub(crate) fn path_maxes_from_timing_rows(
    timings: &[SelectorTimingRecord],
) -> Vec<PathMaxDuration> {
    use std::collections::BTreeMap;
    let mut by_path: BTreeMap<String, (u64, String)> = BTreeMap::new();
    for row in timings {
        let Some(ns) = row.duration_ns else {
            continue;
        };
        let path = row
            .selector
            .split_once("::")
            .map_or(row.selector.as_str(), |(p, _)| p)
            .to_string();
        match by_path.get_mut(&path) {
            Some((max_ns, example)) if ns > *max_ns => {
                *max_ns = ns;
                *example = row.selector.clone();
            }
            Some(_) => {}
            None => {
                by_path.insert(path, (ns, row.selector.clone()));
            }
        }
    }
    by_path
        .into_iter()
        .map(
            |(path, (max_duration_ns, example_selector))| PathMaxDuration {
                path,
                max_duration_ns,
                example_selector,
            },
        )
        .collect()
}

pub(crate) fn path_maxes_from_selector_durations(
    pairs: &[(String, std::time::Duration)],
) -> Vec<PathMaxDuration> {
    use std::collections::BTreeMap;
    let mut by_path: BTreeMap<String, (u64, String)> = BTreeMap::new();
    for (selector, duration) in pairs {
        let ns = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        let path = selector
            .split_once("::")
            .map_or(selector.as_str(), |(p, _)| p)
            .to_string();
        match by_path.get_mut(&path) {
            Some((max_ns, example)) if ns > *max_ns => {
                *max_ns = ns;
                *example = selector.clone();
            }
            Some(_) => {}
            None => {
                by_path.insert(path, (ns, selector.clone()));
            }
        }
    }
    by_path
        .into_iter()
        .map(
            |(path, (max_duration_ns, example_selector))| PathMaxDuration {
                path,
                max_duration_ns,
                example_selector,
            },
        )
        .collect()
}
