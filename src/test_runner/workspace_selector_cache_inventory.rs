use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::analyze_cache::fnv1a64;

use super::digest::{flush_persisted_digests, hash_file_contents};
use super::{LangFingerprints, fresh};

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "target"
            | ".kiss"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".pytest_cache"
            | ".rslip_cache"
            | "node_modules"
    )
}

fn ignored(rel: &str, ignore: &[String]) -> bool {
    kiss::path_ignored_by_prefixes(rel, ignore)
}

fn is_rust_discovery_input(rel: &str) -> bool {
    rel.ends_with(".rs")
        || rel == "Cargo.toml"
        || rel.ends_with("/Cargo.toml")
        || rel == "Cargo.lock"
        || rel.ends_with("/Cargo.lock")
        || rel == ".cargo/config"
        || rel == ".cargo/config.toml"
        || rel.ends_with("/.cargo/config")
        || rel.ends_with("/.cargo/config.toml")
        || rel == "rust-toolchain"
        || rel == "rust-toolchain.toml"
        || rel.ends_with("/rust-toolchain")
        || rel.ends_with("/rust-toolchain.toml")
}

pub(crate) fn rust_selector_inputs_fingerprint_for_cache(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<String> {
    let rels = fresh::rust_source_rels(repo_root, ignore)?;
    let fingerprint = hash_rel_list(b"workspace-selectors-fp-v6-git-rs", repo_root, &rels)?;
    flush_persisted_digests(repo_root);
    Ok(fingerprint)
}

pub(crate) fn workspace_source_inventory_fingerprint_for_cache(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<String> {
    let rels = source_inventory_git(repo_root, ignore)
        .or_else(|_| source_inventory_walk(repo_root, ignore))?;
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, b"workspace-source-inventory-v1");
    for rel in rels {
        h = fnv1a64(h, rel.as_bytes());
        h = fnv1a64(h, &[0]);
        if rel == "Cargo.toml" || rel.ends_with("/Cargo.toml") {
            h = fnv1a64(h, &fs::read(repo_root.join(rel))?);
        }
    }
    Ok(format!("{h:016x}"))
}

fn source_inventory_git(repo_root: &Path, ignore: &[String]) -> io::Result<Vec<String>> {
    let output = kiss::scrubbed_git_command(repo_root)
        .args([
            "ls-files",
            "-z",
            "-c",
            "-o",
            "--exclude-standard",
            "--",
            "*.py",
            "*.rs",
            "Cargo.toml",
            "**/Cargo.toml",
            "Cargo.lock",
            "**/Cargo.lock",
            ".cargo/config",
            ".cargo/config.toml",
            "**/.cargo/config",
            "**/.cargo/config.toml",
            "rust-toolchain",
            "rust-toolchain.toml",
            "**/rust-toolchain",
            "**/rust-toolchain.toml",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("git ls-files failed"));
    }
    let mut rels: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).replace('\\', "/"))
        .filter(|rel| !ignored(rel, ignore))
        .collect();
    rels.sort();
    rels.dedup();
    Ok(rels)
}

fn source_inventory_walk(repo_root: &Path, ignore: &[String]) -> io::Result<Vec<String>> {
    let mut rels = Vec::new();
    let mut stack = vec![repo_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let rel = path
                .strip_prefix(repo_root)
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if path.is_dir() {
                if !should_skip_dir(name) && !ignored(&rel, ignore) {
                    stack.push(path);
                }
                continue;
            }
            let source = path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("rs")
            });
            if !ignored(&rel, ignore) && (source || name == "Cargo.toml") {
                rels.push(rel);
            }
        }
    }
    rels.sort();
    rels.dedup();
    Ok(rels)
}

fn hash_rel_list(seed: &[u8], repo_root: &Path, rels: &[String]) -> io::Result<String> {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, seed);
    for rel in rels {
        h = hash_file_contents(h, rel, repo_root, &repo_root.join(rel))?;
    }
    Ok(format!("{h:016x}"))
}

pub(super) fn workspace_lang_fingerprints_git(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<LangFingerprints> {
    let output = kiss::scrubbed_git_command(repo_root)
        .args([
            "ls-files",
            "-z",
            "-c",
            "-o",
            "--exclude-standard",
            "--",
            "*.py",
            "*.rs",
            "Cargo.toml",
            "**/Cargo.toml",
            "Cargo.lock",
            "**/Cargo.lock",
            ".cargo/config",
            ".cargo/config.toml",
            "**/.cargo/config",
            "**/.cargo/config.toml",
            "rust-toolchain",
            "rust-toolchain.toml",
            "**/rust-toolchain",
            "**/rust-toolchain.toml",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("git ls-files failed"));
    }
    let mut py_rels = Vec::new();
    let mut rs_rels = Vec::new();
    for part in output
        .stdout
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
    {
        let rel = String::from_utf8_lossy(part).replace('\\', "/");
        if ignored(&rel, ignore) {
            continue;
        }
        if rel.ends_with(".py") {
            py_rels.push(rel);
        } else if is_rust_discovery_input(&rel) {
            rs_rels.push(rel);
        }
    }
    py_rels.sort();
    py_rels.dedup();
    rs_rels.sort();
    rs_rels.dedup();
    fresh::remember_source_rels(repo_root, ignore, &py_rels, &rs_rels);
    Ok(LangFingerprints {
        python: hash_rel_list(b"workspace-selectors-fp-v6-git-py", repo_root, &py_rels)?,
        rust: hash_rel_list(b"workspace-selectors-fp-v6-git-rs", repo_root, &rs_rels)?,
    })
}

pub(super) fn workspace_lang_fingerprints_walk(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<LangFingerprints> {
    let mut py_h = fnv1a64(0xcbf2_9ce4_8422_2325, b"workspace-selectors-fp-v6-walk-py");
    let mut rs_h = fnv1a64(0xcbf2_9ce4_8422_2325, b"workspace-selectors-fp-v6-walk-rs");
    let mut py_rels = Vec::new();
    let mut rs_rels = Vec::new();
    let mut stack = vec![repo_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)?;
        let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        let mut hashes = WalkHashes {
            py_h: &mut py_h,
            rs_h: &mut rs_h,
            py_rels: &mut py_rels,
            rs_rels: &mut rs_rels,
        };
        for path in paths {
            hash_walk_path(repo_root, ignore, &path, &mut stack, &mut hashes)?;
        }
    }
    py_rels.sort();
    rs_rels.sort();
    fresh::remember_source_rels(repo_root, ignore, &py_rels, &rs_rels);
    Ok(LangFingerprints {
        python: format!("{py_h:016x}"),
        rust: format!("{rs_h:016x}"),
    })
}

struct WalkHashes<'a> {
    py_h: &'a mut u64,
    rs_h: &'a mut u64,
    py_rels: &'a mut Vec<String>,
    rs_rels: &'a mut Vec<String>,
}

fn hash_walk_path(
    repo_root: &Path,
    ignore: &[String],
    path: &Path,
    stack: &mut Vec<PathBuf>,
    hashes: &mut WalkHashes<'_>,
) -> io::Result<()> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let rel = path
        .strip_prefix(repo_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if path.is_dir() {
        if !should_skip_dir(name) && !ignored(&rel, ignore) {
            stack.push(path.to_path_buf());
        }
        return Ok(());
    }
    if ignored(&rel, ignore) {
        return Ok(());
    }
    let is_py = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("py"));
    let is_rust_input = is_rust_discovery_input(&rel);
    if !is_py && !is_rust_input {
        return Ok(());
    }
    let hashed = hash_file_contents(
        if is_py { *hashes.py_h } else { *hashes.rs_h },
        &rel,
        repo_root,
        path,
    )?;
    if is_py {
        *hashes.py_h = hashed;
        hashes.py_rels.push(rel);
    } else {
        *hashes.rs_h = hashed;
        hashes.rs_rels.push(rel);
    }
    Ok(())
}
