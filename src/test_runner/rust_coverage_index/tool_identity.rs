use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use rust_llvm_cov_runner::RustCoverageToolIdentity;

use crate::test_runner::runners::command_stdout;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RustToolVersionsCache {
    cargo: String,
    llvm_cov: String,
    rustc: String,
    cargo_nextest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ToolIdentityCacheKey {
    cargo: PathBuf,
    rustc: PathBuf,
    cargo_meta: Option<(u64, Option<SystemTime>)>,
    config_metas: Vec<(PathBuf, u64, Option<SystemTime>)>,
    toolchain_metas: Vec<(PathBuf, u64, Option<SystemTime>)>,
}

struct ToolIdentityCache {
    key: ToolIdentityCacheKey,
    tools: RustCoverageToolIdentity,
}

static TOOLS_CACHE: Mutex<Option<ToolIdentityCache>> = Mutex::new(None);

fn rust_tool_versions_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("rust_tool_versions.json")
}

fn read_cached_rust_tool_identity(repo_root: &Path) -> Option<RustCoverageToolIdentity> {
    let bytes = fs::read(rust_tool_versions_cache_path(repo_root)).ok()?;
    let cached: RustToolVersionsCache = serde_json::from_slice(&bytes).ok()?;
    Some(RustCoverageToolIdentity {
        cargo_version: cached.cargo,
        llvm_cov_version: cached.llvm_cov,
        rustc_version: cached.rustc,
        cargo_nextest_version: cached.cargo_nextest,
    })
}

fn write_cached_rust_tool_identity(
    repo_root: &Path,
    tools: &RustCoverageToolIdentity,
) -> std::io::Result<()> {
    let path = rust_tool_versions_cache_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let cached = RustToolVersionsCache {
        cargo: tools.cargo_version.clone(),
        llvm_cov: tools.llvm_cov_version.clone(),
        rustc: tools.rustc_version.clone(),
        cargo_nextest: tools.cargo_nextest_version.clone(),
    };
    let bytes = serde_json::to_vec(&cached).map_err(std::io::Error::other)?;
    fs::write(path, bytes)
}

fn file_meta(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

fn collect_config_metas(repo_root: &Path) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    let mut out = Vec::new();
    for name in ["config", "config.toml"] {
        let path = repo_root.join(".cargo").join(name);
        if let Some((len, mtime)) = file_meta(&path) {
            out.push((path, len, mtime));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn collect_toolchain_metas(repo_root: &Path) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(repo_root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with("rust-toolchain") {
                let path = entry.path();
                if let Some((len, mtime)) = file_meta(&path) {
                    out.push((path, len, mtime));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn build_tool_identity_cache_key(repo_root: &Path) -> ToolIdentityCacheKey {
    let cargo = PathBuf::from("cargo");
    let rustc = PathBuf::from("rustc");
    ToolIdentityCacheKey {
        cargo_meta: which_meta(&cargo),
        config_metas: collect_config_metas(repo_root),
        toolchain_metas: collect_toolchain_metas(repo_root),
        cargo,
        rustc,
    }
}

fn which_meta(program: &Path) -> Option<(u64, Option<SystemTime>)> {
    // Best-effort: if PATH resolution fails, key still includes program name.
    file_meta(program).or_else(|| {
        std::env::var_os("PATH").and_then(|paths| {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join(program);
                if let Some(meta) = file_meta(&candidate) {
                    return Some(meta);
                }
            }
            None
        })
    })
}

fn detect_live_rust_coverage_tool_identity(
    repo_root: &Path,
) -> Result<RustCoverageToolIdentity, String> {
    let cargo = PathBuf::from("cargo");
    let rustc = PathBuf::from("rustc");
    Ok(RustCoverageToolIdentity {
        cargo_version: command_stdout(&cargo, &["--version"], repo_root)?,
        llvm_cov_version: command_stdout(&cargo, &["llvm-cov", "--version"], repo_root)?,
        rustc_version: command_stdout(&rustc, &["-Vv"], repo_root)?,
        cargo_nextest_version: command_stdout(&cargo, &["nextest", "--version"], repo_root)?,
    })
}

fn detect_rust_coverage_tool_identity(
    repo_root: &Path,
) -> Result<RustCoverageToolIdentity, String> {
    let live = detect_live_rust_coverage_tool_identity(repo_root)?;
    if let Some(cached) = read_cached_rust_tool_identity(repo_root)
        && cached == live
    {
        return Ok(cached);
    }
    let _ = write_cached_rust_tool_identity(repo_root, &live);
    Ok(live)
}

pub(crate) fn cached_rust_coverage_tool_identity(
    repo_root: &Path,
) -> Result<RustCoverageToolIdentity, String> {
    let key = build_tool_identity_cache_key(repo_root);
    let mut guard = TOOLS_CACHE.lock().expect("tool identity cache lock");
    if let Some(cached) = guard.as_ref()
        && cached.key == key
    {
        return Ok(cached.tools.clone());
    }
    let tools = detect_rust_coverage_tool_identity(repo_root)?;
    *guard = Some(ToolIdentityCache {
        key,
        tools: tools.clone(),
    });
    Ok(tools)
}

pub(crate) fn rust_coverage_tool_versions_from_cache_or_detect(
    repo_root: &Path,
) -> Result<(String, String, String, String), String> {
    let tools = cached_rust_coverage_tool_identity(repo_root)?;
    Ok((
        tools.cargo_version,
        tools.llvm_cov_version,
        tools.rustc_version,
        tools.cargo_nextest_version,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_cache_changes_when_toolchain_file_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let key1 = build_tool_identity_cache_key(tmp.path());
        std::fs::write(tmp.path().join("rust-toolchain.toml"), "[toolchain]\nchannel = \"1.0\"\n")
            .unwrap();
        let key2 = build_tool_identity_cache_key(tmp.path());
        assert_ne!(key1, key2);
    }
}
