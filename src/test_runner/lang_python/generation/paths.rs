
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::test_runner::python_coverage_index::storage::python_unique_suffix;

pub(crate) fn generations_dir(cache_root: &Path) -> PathBuf {
    cache_root.join("generations")
}

pub(crate) fn generation_dir(cache_root: &Path, generation_id: &str) -> PathBuf {
    generations_dir(cache_root).join(generation_id)
}

pub(crate) fn pointer_path(cache_root: &Path) -> PathBuf {
    cache_root.join("population.json")
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(hasher.finalize().as_slice())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn validate_artifact_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || Path::new(name).is_absolute()
    {
        return Err(format!(
            "error: kiss: invalid Python generation artifact name `{name}`"
        ));
    }
    Ok(())
}

pub(crate) fn create_staging_dir(cache_root: &Path) -> Result<PathBuf, String> {
    let parent = generations_dir(cache_root);
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let staged = parent.join(format!(".staging.{}", python_unique_suffix()));
    if staged.exists() {
        return Err(format!(
            "error: kiss: Python generation staging collision at {}",
            staged.display()
        ));
    }
    fs::create_dir_all(&staged).map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn write_create_new_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("error: kiss: create_new {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("error: kiss: write {}: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("error: kiss: sync_all {}: {e}", path.display()))?;
    Ok(())
}

pub(crate) fn sync_dir(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("error: kiss: open dir {}: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("error: kiss: sync dir {}: {e}", path.display()))?;
    Ok(())
}

pub(crate) fn write_json_artifact<T: serde::Serialize>(
    dir: &Path,
    name: &str,
    value: &T,
) -> Result<(Vec<u8>, String), String> {
    validate_artifact_name(name)?;
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let mut payload = bytes;
    payload.push(b'\n');
    let digest = sha256_hex(&payload);
    write_create_new_bytes(&dir.join(name), &payload)?;
    Ok((payload, digest))
}

pub(crate) fn read_validated_artifact(
    dir: &Path,
    name: &str,
    expected_len: u64,
    expected_sha: &str,
) -> Result<Vec<u8>, String> {
    validate_artifact_name(name)?;
    let bytes = fs::read(dir.join(name)).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != expected_len {
        return Err(format!(
            "error: kiss: Python generation artifact `{name}` length mismatch"
        ));
    }
    if sha256_hex(&bytes) != expected_sha {
        return Err(format!(
            "error: kiss: Python generation artifact `{name}` checksum mismatch"
        ));
    }
    Ok(bytes)
}
