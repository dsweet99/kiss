use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{CACHE_SCHEMA_VERSION, INDEX_SCHEMA_VERSION, RustCoverageIndex};

pub(crate) fn rust_coverage_index_path(repo_root: &Path) -> PathBuf {
    rust_coverage_cache_root(repo_root).join("index.json")
}

pub(crate) fn rust_population_manifest_path(repo_root: &Path) -> PathBuf {
    rust_coverage_cache_root(repo_root).join("population.json")
}

pub(crate) fn rust_coverage_cache_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("rust_llvm_cov_cache")
}

pub(crate) fn command_stdout(program: &Path, args: &[&str], cwd: &Path) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to spawn {}: {e}", program.display()))?;
    if !output.status.success() {
        return Err(command_failure_message(program, &output.stderr));
    }
    Ok(command_output_text(&output.stdout))
}

pub(crate) fn command_failure_message(program: &Path, stderr: &[u8]) -> String {
    format!(
        "error: kiss test: {} failed: {}",
        program.display(),
        String::from_utf8_lossy(stderr).trim()
    )
}

pub(crate) fn command_output_text(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout).trim().to_string()
}

pub(crate) fn load_current_rust_coverage_index(repo_root: &Path) -> Option<RustCoverageIndex> {
    #[derive(Deserialize)]
    struct OnDiskIndex {
        schema_version: String,
        source_root: String,
        entries_fingerprint: String,
        files: RustCoverageIndex,
    }

    let path = rust_coverage_index_path(repo_root);
    let bytes = fs::read(path).ok()?;
    let index: OnDiskIndex = serde_json::from_slice(&bytes).ok()?;
    if index.schema_version != INDEX_SCHEMA_VERSION {
        return None;
    }
    if index.source_root != normalized_repo_root(repo_root) {
        return None;
    }
    let current_fingerprint = entries_fingerprint(&rust_coverage_cache_root(repo_root)).ok()?;
    (index.entries_fingerprint == current_fingerprint).then_some(index.files)
}

pub(crate) fn write_rust_coverage_index(
    repo_root: &Path,
    index: &RustCoverageIndex,
) -> Result<(), String> {
    #[derive(Serialize)]
    struct OnDiskIndex<'a> {
        schema_version: &'a str,
        source_root: String,
        entries_fingerprint: String,
        files: &'a RustCoverageIndex,
    }

    let path = rust_coverage_index_path(repo_root);
    let parent = path
        .parent()
        .ok_or_else(|| "error: kiss test: Rust coverage index path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp_path = parent.join(format!(".index.{}.tmp", unique_suffix()));
    let mut file = create_new_file(&tmp_path).map_err(|e| e.to_string())?;
    let payload = OnDiskIndex {
        schema_version: INDEX_SCHEMA_VERSION,
        source_root: normalized_repo_root(repo_root),
        entries_fingerprint: entries_fingerprint(&rust_coverage_cache_root(repo_root))
            .map_err(|e| e.to_string())?,
        files: index,
    };
    serde_json::to_writer_pretty(&mut file, &payload).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&tmp_path, path).map_err(|e| e.to_string())
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

pub(crate) fn create_new_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

pub(crate) fn repo_relative_coverage_file(repo_root: &Path, file: &str) -> Option<String> {
    repo_relative_path(repo_root, Path::new(file))
}

pub(crate) fn repo_relative_path(repo_root: &Path, path: &Path) -> Option<String> {
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

pub(crate) fn normalized_repo_root(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

pub(crate) fn workspace_input_fingerprint(repo_root: &Path) -> io::Result<String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut h = 0xcbf2_9ce4_8422_2325;
    h = fnv1a64(h, CACHE_SCHEMA_VERSION.as_bytes());
    h = fnv1a64(h, b"rust-workspace-inputs-v1");
    for path in workspace_input_paths(&root)? {
        let rel = path.strip_prefix(&root).unwrap_or(&path);
        h = fnv1a64(h, rel.to_string_lossy().as_bytes());
        h = fnv1a64(h, &[0]);
        h = fnv1a64(h, &fs::read(path)?);
        h = fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

fn workspace_input_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_workspace_inputs(root, &mut out)?;
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    Ok(out)
}

fn visit_workspace_inputs(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_skip_workspace_input_dir(&path) {
                continue;
            }
            visit_workspace_inputs(&path, out)?;
        } else if file_type.is_file() && is_workspace_input_path(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn should_skip_workspace_input_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".git" | "target")
    ) || path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "rust_llvm_cov_cache")
}

fn is_workspace_input_path(path: &Path) -> bool {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
    {
        return true;
    }
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Cargo.toml" | "Cargo.lock" | "config.toml")
    ) || is_cargo_config_input_path(path)
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rust-toolchain"))
}

pub(crate) fn is_cargo_config_input_path(path: &Path) -> bool {
    path_has_cargo_parent(path) && is_cargo_config_file_name(path)
}

pub(crate) fn path_has_cargo_parent(path: &Path) -> bool {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        == Some(".cargo")
}

pub(crate) fn is_cargo_config_file_name(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("config" | "config.toml")
    )
}

pub(crate) fn entries_fingerprint(cache_root: &Path) -> io::Result<String> {
    let mut h = 0xcbf2_9ce4_8422_2325;
    h = fnv1a64(h, CACHE_SCHEMA_VERSION.as_bytes());
    for path in rust_coverage_entry_paths(cache_root) {
        let meta = fs::metadata(&path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        h = fnv1a64(h, name.as_bytes());
        h = fnv1a64(h, &[0]);
        h = fnv1a64(h, meta.len().to_string().as_bytes());
        h = fnv1a64(h, &[0]);
        if let Ok(modified) = meta.modified()
            && let Ok(duration) = modified.duration_since(UNIX_EPOCH)
        {
            h = fnv1a64(h, duration.as_nanos().to_string().as_bytes());
        }
        h = fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

pub(crate) fn fnv1a64(mut h: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0100_0000_01b3;
    for byte in bytes {
        h = fnv1a64_step(h, *byte, PRIME);
    }
    h
}

pub(crate) fn fnv1a64_step(h: u64, byte: u8, prime: u64) -> u64 {
    (h ^ u64::from(byte)).wrapping_mul(prime)
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
