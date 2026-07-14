use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use serde::Serialize;
#[cfg(test)]
use std::io::Write;

#[cfg(test)]
use super::RustCoverageIndex;

#[cfg(test)]
pub(crate) fn rust_coverage_index_path(repo_root: &Path) -> PathBuf {
    rust_coverage_cache_root(repo_root).join("index.json")
}

#[cfg(test)]
pub(crate) fn rust_population_manifest_path(repo_root: &Path) -> PathBuf {
    rust_coverage_cache_root(repo_root).join("population.json")
}

pub(crate) fn rust_coverage_cache_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("rust_llvm_cov_cache")
}

pub(crate) use crate::test_runner::runners::command_stdout;

pub(crate) fn create_new_file(path: &Path) -> io::Result<std::fs::File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(test)]
pub(crate) fn command_failure_message(program: &Path, stderr: &[u8]) -> String {
    format!(
        "error: kiss test: {} failed: {}",
        program.display(),
        String::from_utf8_lossy(stderr).trim()
    )
}

#[cfg(test)]
pub(crate) fn command_output_text(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout).trim().to_string()
}

#[cfg(test)]
pub(crate) fn load_current_rust_coverage_index(
    repo_root: &Path,
    test_args: &[String],
) -> Option<RustCoverageIndex> {
    load_current_rust_population_state(repo_root, None, test_args).map(|state| state.line_index)
}

#[cfg(test)]
pub(crate) fn load_current_rust_population_state(
    repo_root: &Path,
    selectors: Option<&[String]>,
    test_args: &[String],
) -> Option<rust_llvm_cov_runner::RustPopulationState> {
    let identity = super::current_rust_coverage_batch_identity(repo_root, test_args).ok()?;
    rust_llvm_cov_runner::load_current_population_state(
        &rust_coverage_cache_root(repo_root),
        repo_root,
        &identity,
        selectors,
    )
}

#[cfg(test)]
pub(crate) fn write_rust_coverage_index(
    repo_root: &Path,
    index: &RustCoverageIndex,
) -> Result<(), String> {
    let cache_root = rust_coverage_cache_root(repo_root);
    let generation = super::test_support::test_generation_fingerprint(repo_root);
    let entries_fingerprint =
        rust_llvm_cov_runner::generation_entries_fingerprint(&cache_root, &generation)
            .map_err(|e| e.to_string())?;
    let source_root = normalized_repo_root(repo_root);
    let batch_identity =
        super::current_rust_coverage_batch_identity(repo_root, &[]).map_err(|e| e.to_string())?;

    #[derive(Serialize)]
    struct OnDiskIndex<'a> {
        schema_version: &'a str,
        source_root: String,
        generation_fingerprint: String,
        entries_fingerprint: String,
        files: &'a RustCoverageIndex,
    }
    let index_path = rust_coverage_index_path(repo_root);
    let parent = index_path
        .parent()
        .ok_or_else(|| "error: kiss test: Rust coverage index path has no parent".to_string())?
        .to_path_buf();
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    write_test_json_atomically(
        &parent.join(format!(".index.{}.tmp", unique_suffix())),
        &index_path,
        &OnDiskIndex {
            schema_version: rust_llvm_cov_runner::BATCH_INDEX_SCHEMA_VERSION,
            source_root: source_root.clone(),
            generation_fingerprint: generation.clone(),
            entries_fingerprint: entries_fingerprint.clone(),
            files: index,
        },
    )?;

    #[derive(Serialize)]
    struct OnDiskPopulation {
        schema_version: String,
        source_root: String,
        input_fingerprint: String,
        generation_fingerprint: String,
        selection_context_fingerprint: String,
        entries_fingerprint: String,
        selectors: Vec<String>,
        ordinary_source_digests: Vec<serde_json::Value>,
    }
    let population_path = rust_population_manifest_path(repo_root);
    write_test_json_atomically(
        &parent.join(format!(".population.{}.tmp", unique_suffix())),
        &population_path,
        &OnDiskPopulation {
            schema_version: rust_llvm_cov_runner::BATCH_POPULATION_SCHEMA_VERSION.to_string(),
            source_root,
            input_fingerprint: batch_identity.input_digest,
            generation_fingerprint: generation,
            selection_context_fingerprint: batch_identity.selection_context_fingerprint,
            entries_fingerprint,
            selectors: index
                .values()
                .flatten()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            ordinary_source_digests: batch_identity
                .ordinary_source_digests
                .iter()
                .map(|(path, digest)| serde_json::json!({ "path": path, "digest": digest }))
                .collect(),
        },
    )
}

#[cfg(test)]
pub(crate) fn write_test_json_atomically<T: Serialize>(
    tmp_path: &Path,
    path: &Path,
    payload: &T,
) -> Result<(), String> {
    let mut file = create_new_file(tmp_path).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, payload).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(tmp_path, path).map_err(|e| e.to_string())
}

pub(crate) fn rust_coverage_entry_paths(cache_root: &Path) -> Vec<PathBuf> {
    let entries_dir = cache_root.join("entries");
    let Ok(entries) = fs::read_dir(entries_dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    paths.sort();
    paths
}

#[cfg(test)]
pub(crate) fn normalized_repo_root(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
pub(crate) fn workspace_input_fingerprint(repo_root: &Path) -> io::Result<String> {
    rust_llvm_cov_runner::workspace_input_digest(repo_root)
}

pub(crate) fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}.{}", process::id(), nanos)
}

#[cfg(test)]
#[path = "storage_test.rs"]
mod storage_test;
