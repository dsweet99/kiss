//! Ephemeral `.kiss/profraw` sinks for discard LLVM profile dumps.
//!
//! Instrumented processes with no intentional `LLVM_PROFILE_FILE` otherwise write
//! `default_*.profraw` into the process CWD. Kiss redirects those dumps under
//! `.kiss/profraw` and deletes them once the coverage batch no longer needs them.
//! Intentional instance/pool profiles stay on their export paths.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const KISS_PROFRAW_DIR_ENV: &str = "KISS_PROFRAW_DIR";
pub(crate) const DISCARD_PROFILE_PATTERN: &str = "default_%m_%p.profraw";

pub(crate) fn kiss_profraw_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("profraw")
}

pub(crate) fn kiss_profraw_from_cache_root(cache_root: &Path) -> PathBuf {
    cache_root
        .parent()
        .map(|parent| parent.join("profraw"))
        .unwrap_or_else(|| cache_root.join("profraw"))
}

pub(crate) fn repo_root_from_cache_root(cache_root: &Path) -> Option<PathBuf> {
    // cache = repo/.kiss/rust_llvm_cov_cache
    if cache_root.file_name()?.to_str()? != "rust_llvm_cov_cache" {
        return None;
    }
    let kiss = cache_root.parent()?;
    if kiss.file_name()?.to_str()? != ".kiss" {
        return None;
    }
    Some(kiss.parent()?.to_path_buf())
}

pub(crate) fn discard_llvm_profile_path(kiss_profraw: &Path) -> PathBuf {
    kiss_profraw.join(DISCARD_PROFILE_PATTERN)
}

/// Export discard sink env for every coverage batch plan (does not create dirs).
pub(crate) fn ensure_kiss_profraw_env(
    env: &mut std::collections::BTreeMap<String, String>,
    repo_root: &Path,
) {
    let dir = kiss_profraw_dir(repo_root);
    env.insert(
        KISS_PROFRAW_DIR_ENV.to_string(),
        dir.to_string_lossy().into_owned(),
    );
    env.insert(
        "LLVM_PROFILE_FILE".to_string(),
        discard_llvm_profile_path(&dir).to_string_lossy().into_owned(),
    );
}

pub(crate) fn ensure_kiss_profraw(kiss_profraw: &Path) -> io::Result<()> {
    fs::create_dir_all(kiss_profraw)
}

pub(crate) fn resolve_kiss_profraw(output_dir: &Path) -> PathBuf {
    if let Some(value) = std::env::var_os(KISS_PROFRAW_DIR_ENV) {
        return PathBuf::from(value);
    }
    if let Some(dir) = kiss_profraw_from_target_runner_output_dir(output_dir) {
        return dir;
    }
    output_dir.join("kiss-profraw")
}

fn kiss_profraw_from_target_runner_output_dir(output_dir: &Path) -> Option<PathBuf> {
    // repo/.kiss/rust_llvm_cov_cache/runs/<run>/instances
    let run = output_dir.parent()?;
    let runs = run.parent()?;
    let cache = runs.parent()?;
    if cache.file_name()?.to_str()? != "rust_llvm_cov_cache" {
        return None;
    }
    Some(kiss_profraw_from_cache_root(cache))
}

/// Point this process's LLVM profile sink at `.kiss/profraw` (discard only).
pub(crate) fn redirect_llvm_profile_file_to_kiss_profraw(
    kiss_profraw: &Path,
) -> io::Result<PathBuf> {
    ensure_kiss_profraw(kiss_profraw)?;
    let path = discard_llvm_profile_path(kiss_profraw);
    // SAFETY: process-local discard sink before spawning intentional children.
    unsafe {
        std::env::set_var("LLVM_PROFILE_FILE", &path);
    }
    Ok(path)
}

pub(crate) fn redirect_inherited_llvm_profile_file(output_dir: &Path) -> io::Result<()> {
    redirect_llvm_profile_file_to_kiss_profraw(&resolve_kiss_profraw(output_dir)).map(|_| ())
}

/// Walk up from `start` for `.git`; fall back to the absolute start directory.
pub fn discover_repo_root(start: &Path) -> PathBuf {
    let start = start
        .canonicalize()
        .unwrap_or_else(|_| start.to_path_buf());
    let start_dir = if start.is_file() {
        start.parent().unwrap_or(&start).to_path_buf()
    } else {
        start
    };
    let mut cursor = start_dir.as_path();
    loop {
        if cursor.join(".git").exists() {
            return cursor.to_path_buf();
        }
        let Some(parent) = cursor.parent() else {
            return start_dir;
        };
        cursor = parent;
    }
}

/// Unconditionally redirect this process to absolute `<repo>/.kiss/profraw`.
///
/// LLVM reads `LLVM_PROFILE_FILE` in `__llvm_profile_initialize` before `main`,
/// so a late `set_var` alone does not move dump-at-exit. When the env is not
/// already the absolute discard path, set it and re-exec (skipped under
/// `cfg(test)` so unit tests do not restart the harness).
pub fn redirect_this_process(repo_root: &Path) -> io::Result<PathBuf> {
    let repo_root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let kiss_profraw = kiss_profraw_dir(&repo_root);
    let path = discard_llvm_profile_path(&kiss_profraw);
    let already_redirected = llvm_profile_file_is(&path);
    let path = redirect_llvm_profile_file_to_kiss_profraw(&kiss_profraw)?;
    // SAFETY: process-local discard metadata for children / later sweeps.
    unsafe {
        std::env::set_var(KISS_PROFRAW_DIR_ENV, &kiss_profraw);
    }
    if !cfg!(test) && !already_redirected {
        reexec_current_process()?;
    }
    Ok(path)
}

fn llvm_profile_file_is(path: &Path) -> bool {
    std::env::var_os("LLVM_PROFILE_FILE").is_some_and(|value| Path::new(&value) == path)
}

fn reexec_current_process() -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let exe = std::env::current_exe()?;
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let error = Command::new(exe).args(args).exec();
        Err(io::Error::other(format!("re-exec after profraw redirect failed: {error}")))
    }
    #[cfg(not(unix))]
    {
        Ok(())
    }
}

/// Delete leftover discard `*.profraw` under `<repo>/.kiss/profraw`.
///
/// Keeps the directory itself so this process's LLVM dump-at-exit still has a sink.
pub fn sweep_kiss_profraw_dir(repo_root: &Path) -> io::Result<()> {
    let kiss_profraw = kiss_profraw_dir(repo_root);
    let entries = match fs::read_dir(&kiss_profraw) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        remove_if_profraw(&entry?.path())?;
    }
    Ok(())
}

/// Best-effort pid-scoped cleanup of the discard sink (does not beat LLVM atexit).
pub struct KissProfrawProcessGuard {
    kiss_profraw: PathBuf,
    pid: u32,
}

impl KissProfrawProcessGuard {
    pub fn for_current_process(repo_root: &Path) -> Self {
        Self {
            kiss_profraw: kiss_profraw_dir(repo_root),
            pid: std::process::id(),
        }
    }
}

impl Drop for KissProfrawProcessGuard {
    fn drop(&mut self) {
        let _ = cleanup_kiss_profraw_for_pid(&self.kiss_profraw, self.pid);
    }
}

/// Delete discard dumps for one writer pid (`…_{pid}.profraw`); concurrency-safe.
pub(crate) fn cleanup_kiss_profraw_for_pid(kiss_profraw: &Path, pid: u32) -> io::Result<()> {
    let suffix = format!("_{pid}.profraw");
    let entries = match fs::read_dir(kiss_profraw) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(&suffix) {
            ignore_not_found(fs::remove_file(&path))?;
        }
    }
    Ok(())
}

/// Delete all discard `*.profraw` under `.kiss/profraw`; remove the dir if empty.
pub(crate) fn cleanup_kiss_profraw(kiss_profraw: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(kiss_profraw) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        remove_if_profraw(&entry?.path())?;
    }
    ignore_absent_or_nonempty(fs::remove_dir(kiss_profraw))
}

/// Batch-begin orphan sweep: repo root, immediate `crates/<pkg>/`, leftover `.kiss/tmp`.
pub(crate) fn sweep_orphan_default_profraw(repo_root: &Path) -> io::Result<()> {
    delete_default_profraw_in_dir(repo_root)?;
    let crates_dir = repo_root.join("crates");
    match fs::read_dir(&crates_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    delete_default_profraw_in_dir(&entry.path())?;
                }
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let legacy_tmp = repo_root.join(".kiss").join("tmp");
    if legacy_tmp.is_dir() {
        delete_default_profraw_in_dir(&legacy_tmp)?;
        ignore_absent_or_nonempty(fs::remove_dir(&legacy_tmp))?;
    }
    Ok(())
}

fn delete_default_profraw_in_dir(dir: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("default_") && name.ends_with(".profraw") && path.is_file() {
            ignore_not_found(fs::remove_file(&path))?;
        }
    }
    Ok(())
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
#[path = "kiss_profraw_test.rs"]
mod tests;

#[cfg(test)]
#[path = "kiss_profraw_cleanup_test.rs"]
mod cleanup_tests;
