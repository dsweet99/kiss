use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::paths::{generation_dir, pointer_path, read_validated_artifact, sha256_hex};
use super::types::{
    CoveredLinesMap, GENERATION_SCHEMA_VERSION, GenerationManifest, InternedLineIndex,
    POINTER_SCHEMA_VERSION, PinnedPythonGeneration, PopulationPointer, SelectorCoverageMap,
    SelectorTimingRecord, decode_line_index_bytes,
};
use crate::test_runner::python_coverage_index::storage::python_coverage_cache_root;

#[derive(Clone, Debug)]
pub(crate) enum GenerationLoadError {
    MissingOrStale,
    Corrupt(String),
}

pub(crate) fn try_load_pinned_python_generation(
    repo_root: &Path,
) -> Result<PinnedPythonGeneration, GenerationLoadError> {
    let cache_root = python_coverage_cache_root(repo_root).map_err(GenerationLoadError::Corrupt)?;
    let _guard = kiss::rslip::lock_rslip_derived_state(&cache_root)
        .map_err(|e| GenerationLoadError::Corrupt(e.to_string()))?;
    load_pinned_locked(&cache_root)
}

pub(crate) fn load_pinned_locked(
    cache_root: &Path,
) -> Result<PinnedPythonGeneration, GenerationLoadError> {
    let pointer = read_pointer(cache_root)?;
    let gen_dir = generation_dir(cache_root, &pointer.generation_id);
    let manifest_bytes =
        fs::read(gen_dir.join("manifest.json")).map_err(|_| GenerationLoadError::MissingOrStale)?;
    if sha256_hex(&manifest_bytes) != pointer.manifest_sha256 {
        return Err(GenerationLoadError::Corrupt(
            "manifest checksum mismatch".to_string(),
        ));
    }
    let manifest: GenerationManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| GenerationLoadError::Corrupt(e.to_string()))?;
    if manifest.schema_version != GENERATION_SCHEMA_VERSION
        || manifest.generation_id != pointer.generation_id
    {
        return Err(GenerationLoadError::MissingOrStale);
    }
    let coverage = read_artifact_json::<CoveredLinesMap>(&gen_dir, &manifest, "coverage.json")?;
    let timings =
        read_artifact_json::<Vec<SelectorTimingRecord>>(&gen_dir, &manifest, "timings.json")?;
    let line_index = read_line_index(&gen_dir, &manifest)?;
    let selector_coverage =
        read_artifact_json::<SelectorCoverageMap>(&gen_dir, &manifest, "selector_coverage.json")?;
    Ok(PinnedPythonGeneration {
        generation_id: manifest.generation_id,
        plan: manifest.plan,
        complete: manifest.complete,
        coverage,
        timings,
        line_index,
        selector_coverage,
    })
}

pub(crate) fn try_load_pinned_python_generation_warm(
    repo_root: &Path,
) -> Result<PinnedPythonGeneration, GenerationLoadError> {
    super::memo::try_load_pinned_python_generation_warm_memoized(repo_root)
}

pub(crate) fn pinned_python_generation_artifacts_present(repo_root: &Path) -> bool {
    let Ok(cache_root) = python_coverage_cache_root(repo_root) else {
        return false;
    };
    let Ok(pointer) = read_pointer(&cache_root) else {
        return false;
    };
    let gen_dir = generation_dir(&cache_root, &pointer.generation_id);
    gen_dir.join("manifest.json").is_file()
        && gen_dir.join("coverage.json").is_file()
        && gen_dir.join("timings.json").is_file()
}

pub(crate) fn load_pinned_warm_locked(
    cache_root: &Path,
) -> Result<PinnedPythonGeneration, GenerationLoadError> {
    let pointer = read_pointer(cache_root)?;
    let gen_dir = generation_dir(cache_root, &pointer.generation_id);
    let manifest_bytes =
        fs::read(gen_dir.join("manifest.json")).map_err(|_| GenerationLoadError::MissingOrStale)?;
    if sha256_hex(&manifest_bytes) != pointer.manifest_sha256 {
        return Err(GenerationLoadError::Corrupt(
            "manifest checksum mismatch".to_string(),
        ));
    }
    let manifest: GenerationManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| GenerationLoadError::Corrupt(e.to_string()))?;
    if manifest.schema_version != GENERATION_SCHEMA_VERSION
        || manifest.generation_id != pointer.generation_id
    {
        return Err(GenerationLoadError::MissingOrStale);
    }
    let coverage = read_artifact_json::<CoveredLinesMap>(&gen_dir, &manifest, "coverage.json")?;
    let timings =
        read_artifact_json::<Vec<SelectorTimingRecord>>(&gen_dir, &manifest, "timings.json")?;
    Ok(PinnedPythonGeneration {
        generation_id: manifest.generation_id,
        plan: manifest.plan,
        complete: manifest.complete,
        coverage,
        timings,
        line_index: InternedLineIndex::default(),
        selector_coverage: BTreeMap::new(),
    })
}

fn read_pointer(cache_root: &Path) -> Result<PopulationPointer, GenerationLoadError> {
    let bytes =
        fs::read(pointer_path(cache_root)).map_err(|_| GenerationLoadError::MissingOrStale)?;
    let pointer: PopulationPointer =
        serde_json::from_slice(&bytes).map_err(|_| GenerationLoadError::MissingOrStale)?;
    if pointer.schema_version != POINTER_SCHEMA_VERSION {
        return Err(GenerationLoadError::MissingOrStale);
    }
    Ok(pointer)
}

fn read_artifact_json<T: for<'de> Deserialize<'de>>(
    gen_dir: &Path,
    manifest: &GenerationManifest,
    name: &str,
) -> Result<T, GenerationLoadError> {
    let meta = manifest
        .artifacts
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| GenerationLoadError::Corrupt(format!("missing artifact meta `{name}`")))?;
    let bytes = read_validated_artifact(gen_dir, name, meta.byte_length, &meta.sha256)
        .map_err(GenerationLoadError::Corrupt)?;
    serde_json::from_slice(&bytes).map_err(|e| GenerationLoadError::Corrupt(e.to_string()))
}

fn read_line_index(
    gen_dir: &Path,
    manifest: &GenerationManifest,
) -> Result<InternedLineIndex, GenerationLoadError> {
    let meta = manifest
        .artifacts
        .iter()
        .find(|a| a.name == "line_index.json")
        .ok_or_else(|| {
            GenerationLoadError::Corrupt("missing artifact meta `line_index.json`".to_string())
        })?;
    let bytes = read_validated_artifact(gen_dir, "line_index.json", meta.byte_length, &meta.sha256)
        .map_err(GenerationLoadError::Corrupt)?;
    decode_line_index_bytes(&bytes).map_err(GenerationLoadError::Corrupt)
}

pub(crate) fn try_load_pinned_python_generation_without_line_index(
    repo_root: &Path,
) -> Result<PinnedPythonGeneration, GenerationLoadError> {
    let cache_root = python_coverage_cache_root(repo_root).map_err(GenerationLoadError::Corrupt)?;
    let _guard = kiss::rslip::lock_rslip_derived_state(&cache_root)
        .map_err(|e| GenerationLoadError::Corrupt(e.to_string()))?;
    load_pinned_without_line_index_locked(&cache_root)
}

pub(crate) fn load_pinned_without_line_index_locked(
    cache_root: &Path,
) -> Result<PinnedPythonGeneration, GenerationLoadError> {
    let pointer = read_pointer(cache_root)?;
    let gen_dir = generation_dir(cache_root, &pointer.generation_id);
    let manifest_bytes =
        fs::read(gen_dir.join("manifest.json")).map_err(|_| GenerationLoadError::MissingOrStale)?;
    if sha256_hex(&manifest_bytes) != pointer.manifest_sha256 {
        return Err(GenerationLoadError::Corrupt(
            "manifest checksum mismatch".to_string(),
        ));
    }
    let manifest: GenerationManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| GenerationLoadError::Corrupt(e.to_string()))?;
    if manifest.schema_version != GENERATION_SCHEMA_VERSION
        || manifest.generation_id != pointer.generation_id
    {
        return Err(GenerationLoadError::MissingOrStale);
    }
    let coverage = read_artifact_json::<CoveredLinesMap>(&gen_dir, &manifest, "coverage.json")?;
    let timings =
        read_artifact_json::<Vec<SelectorTimingRecord>>(&gen_dir, &manifest, "timings.json")?;
    let selector_coverage =
        read_artifact_json::<SelectorCoverageMap>(&gen_dir, &manifest, "selector_coverage.json")?;
    Ok(PinnedPythonGeneration {
        generation_id: manifest.generation_id,
        plan: manifest.plan,
        complete: manifest.complete,
        coverage,
        timings,
        line_index: InternedLineIndex::default(),
        selector_coverage,
    })
}

pub(crate) fn file_index_from_selector_coverage(
    selector_coverage: &SelectorCoverageMap,
) -> BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut files: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for (selector, coverage) in selector_coverage {
        for file in coverage.keys() {
            files
                .entry(file.clone())
                .or_default()
                .insert(selector.clone());
        }
    }
    files
}

pub(crate) fn generation_file_index(
    pinned: &PinnedPythonGeneration,
) -> BTreeMap<String, std::collections::BTreeSet<String>> {
    file_index_from_selector_coverage(&pinned.selector_coverage)
}
