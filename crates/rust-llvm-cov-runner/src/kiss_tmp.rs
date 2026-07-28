//! Ephemeral `.kiss/tmp` sinks for discard LLVM profile dumps.
//!
//! Instrumented processes with no intentional `LLVM_PROFILE_FILE` otherwise write
//! `default_*.profraw` into the process CWD. Kiss redirects those dumps under
//! `.kiss/tmp` and deletes them once the coverage batch no longer needs them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const KISS_TMP_ENV: &str = "KISS_TMP";
pub(crate) const DISCARD_PROFILE_PATTERN: &str = "default_%m_%p.profraw";

pub(crate) fn kiss_tmp_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("tmp")
}

pub(crate) fn kiss_tmp_from_cache_root(cache_root: &Path) -> PathBuf {
    cache_root
        .parent()
        .map(|parent| parent.join("tmp"))
        .unwrap_or_else(|| cache_root.join("tmp"))
}

pub(crate) fn discard_llvm_profile_path(kiss_tmp: &Path) -> PathBuf {
    kiss_tmp.join(DISCARD_PROFILE_PATTERN)
}

pub(crate) fn ensure_kiss_tmp_env(
    env: &mut std::collections::BTreeMap<String, String>,
    repo_root: &Path,
) {
    env.insert(
        KISS_TMP_ENV.to_string(),
        kiss_tmp_dir(repo_root).to_string_lossy().into_owned(),
    );
}

pub(crate) fn ensure_kiss_tmp(kiss_tmp: &Path) -> io::Result<()> {
    fs::create_dir_all(kiss_tmp)
}

pub(crate) fn resolve_kiss_tmp(output_dir: &Path) -> PathBuf {
    if let Some(value) = std::env::var_os(KISS_TMP_ENV) {
        return PathBuf::from(value);
    }
    if let Some(tmp) = kiss_tmp_from_target_runner_output_dir(output_dir) {
        return tmp;
    }
    output_dir.join("kiss-tmp")
}

fn kiss_tmp_from_target_runner_output_dir(output_dir: &Path) -> Option<PathBuf> {
    // repo/.kiss/rust_llvm_cov_cache/runs/<run>/instances
    let run = output_dir.parent()?;
    let runs = run.parent()?;
    let cache = runs.parent()?;
    if cache.file_name()?.to_str()? != "rust_llvm_cov_cache" {
        return None;
    }
    Some(kiss_tmp_from_cache_root(cache))
}

/// Point this process's LLVM profile sink at `.kiss/tmp` (discard only).
pub(crate) fn redirect_llvm_profile_file_to_kiss_tmp(kiss_tmp: &Path) -> io::Result<PathBuf> {
    ensure_kiss_tmp(kiss_tmp)?;
    let path = discard_llvm_profile_path(kiss_tmp);
    // SAFETY: process-local discard sink before spawning intentional children.
    unsafe {
        std::env::set_var("LLVM_PROFILE_FILE", &path);
    }
    Ok(path)
}

pub(crate) fn redirect_inherited_llvm_profile_file(output_dir: &Path) -> io::Result<()> {
    redirect_llvm_profile_file_to_kiss_tmp(&resolve_kiss_tmp(output_dir)).map(|_| ())
}

/// Delete discard `*.profraw` under `.kiss/tmp` once unused; remove the dir if empty.
pub(crate) fn cleanup_kiss_tmp_profraw(kiss_tmp: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(kiss_tmp) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        remove_if_profraw(&entry?.path())?;
    }
    ignore_absent_or_nonempty(fs::remove_dir(kiss_tmp))
}

fn remove_if_profraw(path: &Path) -> io::Result<()> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("profraw") {
        return Ok(());
    }
    ignore_not_found(fs::remove_file(path))
}

fn ignore_not_found(result: io::Result<()>) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn ignore_absent_or_nonempty(result: io::Result<()>) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
#[path = "kiss_tmp_test.rs"]
mod tests;
