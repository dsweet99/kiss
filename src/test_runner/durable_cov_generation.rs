//! Durable coverage generations outside `.kiss`.
//!
//! `rm -rf .kiss` clears the working lease. A matching generation under
//! `target/kiss-cov-durable/` can be rehydrated so cold `kiss cov` need not
//! re-run the full population. Hydration runs only when `.kiss` is absent so
//! intentional partial cache wipes (QA publication / throughput) still refresh.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::analyze_cache::fnv1a64;
use crate::test_runner::check_line_coverage::RequiredCoverageLanguages;
use crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity;

const DURABLE_SCHEMA: &str = "kiss-durable-cov-v1";
const DURABLE_DIR_NAME: &str = "kiss-cov-durable";

pub(crate) fn try_hydrate_if_kiss_absent(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
) -> bool {
    if repo_root.join(".kiss").exists() {
        return false;
    }
    // Prefer the published HEAD pointer so cold hydrate does not recompute the
    // full Rust batch identity (often multi-second). Records-cache / snapshot
    // validation after copy rejects a stale pointer.
    if let Some(key) = read_durable_head(repo_root, required, ignore)
        && hydrate_from_key(repo_root, &key)
    {
        return true;
    }
    let Some(key) = durable_key(repo_root, required, ignore) else {
        return false;
    };
    hydrate_from_key(repo_root, &key)
}

fn hydrate_from_key(repo_root: &Path, key: &str) -> bool {
    let src = durable_generation_dir(repo_root, key);
    if !src.is_dir() {
        return false;
    }
    let dest = repo_root.join(".kiss");
    match copy_dir_recursive(&src, &dest) {
        Ok(()) => {
            eprintln!("kiss cov: hydrated durable coverage generation ({key})");
            true
        }
        Err(_) => {
            let _ = fs::remove_dir_all(&dest);
            false
        }
    }
}

pub(crate) fn publish_durable_generation(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
) {
    let Some(key) = durable_key(repo_root, required, ignore) else {
        return;
    };
    let kiss = repo_root.join(".kiss");
    if !kiss.is_dir() {
        return;
    }
    let dest_root = durable_root(repo_root);
    let staging = dest_root.join(format!(".staging-{key}"));
    let final_dir = durable_generation_dir(repo_root, &key);
    let _ = fs::remove_dir_all(&staging);
    if fs::create_dir_all(&staging).is_err() {
        return;
    }
    if copy_selected_kiss_artifacts(&kiss, &staging, required).is_err() {
        let _ = fs::remove_dir_all(&staging);
        return;
    }
    let _ = fs::create_dir_all(&dest_root);
    let _ = fs::remove_dir_all(&final_dir);
    if fs::rename(&staging, &final_dir).is_err() {
        let _ = fs::remove_dir_all(&staging);
        return;
    }
    write_durable_head(repo_root, required, ignore, &key);
}

fn durable_root(_repo_root: &Path) -> PathBuf {
    // Keep durable generations outside the repo tree. A repo-local `target/`
    // lease would be gathered as Python source and fail coverage gates.
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("kiss").join(DURABLE_DIR_NAME)
}

fn durable_generation_dir(repo_root: &Path, key: &str) -> PathBuf {
    durable_root(repo_root).join(key)
}

fn durable_head_path(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
) -> PathBuf {
    durable_root(repo_root)
        .join("heads")
        .join(format!("{}.head", lease_slot(repo_root, required, ignore)))
}

fn lease_slot(repo_root: &Path, required: RequiredCoverageLanguages, ignore: &[String]) -> String {
    let canonical = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, DURABLE_SCHEMA.as_bytes());
    h = fnv1a64(h, b"lease-slot-v1");
    h = fnv1a64(h, canonical.to_string_lossy().as_bytes());
    h = fnv1a64(h, &[u8::from(required.python), u8::from(required.rust)]);
    for prefix in ignore {
        h = fnv1a64(h, prefix.as_bytes());
        h = fnv1a64(h, &[0]);
    }
    format!("{h:016x}")
}

fn read_durable_head(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
) -> Option<String> {
    let raw = fs::read_to_string(durable_head_path(repo_root, required, ignore)).ok()?;
    let key = raw.trim();
    if key.is_empty() || key.chars().any(|c| !c.is_ascii_hexdigit()) {
        return None;
    }
    Some(key.to_string())
}

fn write_durable_head(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
    key: &str,
) {
    let path = durable_head_path(repo_root, required, ignore);
    if let Some(parent) = path.parent()
        && fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let tmp = path.with_extension("head.tmp");
    if fs::write(&tmp, key.as_bytes()).is_ok() {
        let _ = fs::rename(tmp, path);
    }
}

fn durable_key(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
) -> Option<String> {
    let canonical = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, DURABLE_SCHEMA.as_bytes());
    h = fnv1a64(h, canonical.to_string_lossy().as_bytes());
    h = fnv1a64(h, &[u8::from(required.python), u8::from(required.rust)]);
    for prefix in ignore {
        h = fnv1a64(h, prefix.as_bytes());
        h = fnv1a64(h, &[0]);
    }
    if required.rust {
        let identity = current_rust_coverage_batch_identity(repo_root, &[]).ok()?;
        h = fnv1a64(h, identity.input_digest.as_bytes());
        h = fnv1a64(h, identity.generation_fingerprint.as_bytes());
        h = fnv1a64(h, identity.selection_context_fingerprint.as_bytes());
    }
    if required.python {
        h = fnv1a64(h, b"python-lease-v1");
        // Bind to interpreter identity so a durable Python lease cannot outlive
        // a toolchain change that would invalidate rslip outcomes.
        if let Ok((py, pytest)) = crate::test_runner::runners::detect_rslip_versions(repo_root) {
            h = fnv1a64(h, py.as_bytes());
            h = fnv1a64(h, pytest.as_bytes());
        }
    }
    Some(format!("{h:016x}"))
}

fn copy_selected_kiss_artifacts(
    kiss: &Path,
    dest: &Path,
    required: RequiredCoverageLanguages,
) -> io::Result<()> {
    if required.rust {
        let rust_src = kiss.join("rust_llvm_cov_cache");
        let rust_dest = dest.join("rust_llvm_cov_cache");
        fs::create_dir_all(&rust_dest)?;
        for name in ["check_aggregate.json", "population.json"] {
            let from = rust_src.join(name);
            if from.is_file() {
                link_or_copy(&from, &rust_dest.join(name))?;
            }
        }
    }
    if required.python {
        let py_src = kiss.join("rslip_cache");
        if py_src.is_dir() {
            copy_dir_recursive(&py_src, &dest.join("rslip_cache"))?;
        }
    }
    let records = kiss.join("cov_records_cache.json");
    if records.is_file() {
        link_or_copy(&records, &dest.join("cov_records_cache.json"))?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_file() {
            link_or_copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Prefer hardlinks so Python entry mtimes (used in population fingerprints)
/// survive publish/hydrate. Fall back to copy across devices.
fn link_or_copy(from: &Path, to: &Path) -> io::Result<()> {
    if to.exists() {
        let _ = fs::remove_file(to);
    }
    match fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        Err(_) => fs::copy(from, to).map(|_| ()),
    }
}

#[cfg(test)]
#[path = "durable_cov_generation_test.rs"]
mod tests;
