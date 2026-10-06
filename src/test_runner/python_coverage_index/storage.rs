use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::fs::{File, OpenOptions};

use crate::test_runner::python_cache_path::python_rslip_cache_root;

use super::PythonCoverageIndex;

pub(crate) fn python_coverage_cache_root(repo_root: &Path) -> Result<PathBuf, String> {
    python_rslip_cache_root(repo_root)
}

pub(crate) fn python_coverage_index_file_present(repo_root: &Path) -> bool {
    !python_coverage_entry_paths(repo_root).is_empty()
}

pub(crate) fn python_population_manifest_path(repo_root: &Path) -> Result<PathBuf, String> {
    Ok(python_coverage_cache_root(repo_root)?.join("population.json"))
}

/// The tests whose records cover each repo-relative file; `None` without records.
pub(crate) fn load_current_python_coverage_index(repo_root: &Path) -> Option<PythonCoverageIndex> {
    let paths = python_coverage_entry_paths(repo_root);
    if paths.is_empty() {
        return None;
    }
    let test_file_exists = |test_id: &str| {
        let file = test_id.split_once("::").map_or(test_id, |(file, _)| file);
        repo_root.join(file).is_file()
    };
    let mut index = PythonCoverageIndex::new();
    for record in paths
        .iter()
        .filter_map(|path| kiss::test_records::read_record(path))
        .filter(|record| test_file_exists(&record.test_id))
    {
        for (file, lines) in &record.covered {
            if lines.is_empty() {
                continue;
            }
            if let Some(rel) = python_repo_relative_coverage_file(repo_root, file)
                && !kiss::is_python_test_module_path(Path::new(&rel))
            {
                index.entry(rel).or_default().insert(record.test_id.clone());
            }
        }
    }
    Some(index)
}

pub(crate) fn python_coverage_entry_paths(repo_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(kiss::rslip::python_records_dir(repo_root)) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
}

pub(crate) fn python_entries_fingerprint(repo_root: &Path) -> io::Result<String> {
    let mut h = 0xcbf2_9ce4_8422_2325;
    h = python_fnv1a64(h, kiss::test_records::RECORD_SCHEMA.as_bytes());
    for path in python_coverage_entry_paths(repo_root) {
        let meta = fs::metadata(&path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        h = python_fnv1a64(h, name.as_bytes());
        h = python_fnv1a64(h, &[0]);
        h = python_fnv1a64(h, meta.len().to_string().as_bytes());
        h = python_fnv1a64(h, &[0]);
        if let Ok(modified) = meta.modified()
            && let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            h = python_fnv1a64(h, duration.as_nanos().to_string().as_bytes());
        }
        h = python_fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

pub(crate) fn python_source_input_fingerprint(repo_root: &Path) -> io::Result<String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut h = 0xcbf2_9ce4_8422_2325;
    h = python_fnv1a64(h, kiss::rslip::CACHE_SCHEMA_VERSION.as_bytes());
    h = python_fnv1a64(h, b"python-workspace-inputs-v1");
    for path in python_source_input_paths(&root)? {
        let rel = path.strip_prefix(&root).unwrap_or(&path);
        h = python_fnv1a64(h, rel.to_string_lossy().as_bytes());
        h = python_fnv1a64(h, &[0]);
        h = python_fnv1a64(h, &fs::read(path)?);
        h = python_fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

pub(crate) fn python_source_input_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_python_source_inputs(root, &mut out)?;
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    Ok(out)
}

fn visit_python_source_inputs(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_skip_python_source_input_dir(&path) {
                continue;
            }
            visit_python_source_inputs(&path, out)?;
        } else if file_type.is_file() && is_python_source_input_path(&path) {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn should_skip_python_source_input_dir(path: &Path) -> bool {
    kiss::rslip::should_skip_rslip_dir(path)
}

#[cfg(test)]
pub(crate) fn is_kiss_rslip_cache_dir(path: &Path) -> bool {
    kiss::rslip::is_kiss_rslip_cache_dir(path)
}

pub(crate) fn is_python_source_input_path(path: &Path) -> bool {
    kiss::rslip::is_rslip_cache_input(path)
}

pub(crate) fn python_repo_relative_coverage_file(repo_root: &Path, file: &str) -> Option<String> {
    let rel = python_repo_relative_path(repo_root, Path::new(file))?;
    is_python_indexable_coverage_rel(&rel).then_some(rel)
}

fn is_python_indexable_coverage_rel(rel: &str) -> bool {
    rel.ends_with(".py") && !rel.starts_with(".kiss/") && !rel.starts_with('<')
}

pub(crate) fn python_repo_relative_path(repo_root: &Path, path: &Path) -> Option<String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let candidate = if path.is_absolute() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        let joined = root.join(path);
        joined.canonicalize().unwrap_or(joined)
    };
    let rel = candidate.strip_prefix(&root).ok()?;
    if rel.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(rel.to_string_lossy().replace('\\', "/"))
}

pub(crate) fn normalized_python_repo_root(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
pub(crate) fn write_python_record_fixture(
    repo_root: &Path,
    selector: &str,
    status: kiss::rpytest_runner::TestStatus,
    coverage: kiss::rslip::LineCoverage,
) -> PathBuf {
    let dir = kiss::rslip::python_records_dir(repo_root);
    let record = kiss::test_records::TestRecord {
        schema: kiss::test_records::RECORD_SCHEMA.to_string(),
        language: "python".to_string(),
        test_id: selector.to_string(),
        identity: "fixture".to_string(),
        deps: std::collections::BTreeMap::new(),
        status,
        exit_code: Some(0),
        duration: std::time::Duration::from_millis(1),
        covered: coverage.files,
    };
    kiss::test_records::store_record(&dir, &record).unwrap();
    kiss::test_records::record_path(&dir, selector)
}

#[cfg(test)]
pub(crate) fn create_new_python_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

pub(crate) fn python_unique_suffix() -> String {
    kiss::kiss_publication_barrier::unique_process_suffix()
}

pub(crate) fn python_fnv1a64(h: u64, bytes: &[u8]) -> u64 {
    crate::analyze_cache::fnv1a64(h, bytes)
}

#[cfg(test)]
#[path = "storage_test.rs"]
mod storage_test;
