use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rust_llvm_cov_runner::RustCoverageToolIdentity;

use super::command_stdout;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RustToolVersionsCache {
    cargo: String,
    llvm_cov: String,
    rustc: String,
    cargo_nextest: String,
}

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

fn detect_rust_coverage_tool_identity(
    repo_root: &Path,
) -> Result<RustCoverageToolIdentity, String> {
    if let Some(cached) = read_cached_rust_tool_identity(repo_root) {
        return Ok(cached);
    }
    let cargo = PathBuf::from("cargo");
    let rustc = PathBuf::from("rustc");
    let tools = RustCoverageToolIdentity {
        cargo_version: command_stdout(&cargo, &["--version"], repo_root)?,
        llvm_cov_version: command_stdout(&cargo, &["llvm-cov", "--version"], repo_root)?,
        rustc_version: command_stdout(&rustc, &["-Vv"], repo_root)?,
        cargo_nextest_version: command_stdout(&cargo, &["nextest", "--version"], repo_root)?,
    };
    let _ = write_cached_rust_tool_identity(repo_root, &tools);
    Ok(tools)
}

pub(crate) fn cached_rust_coverage_tool_identity(
    repo_root: &Path,
) -> Result<RustCoverageToolIdentity, String> {
    static TOOLS: OnceLock<RustCoverageToolIdentity> = OnceLock::new();
    if let Some(tools) = TOOLS.get() {
        return Ok(tools.clone());
    }
    let tools = detect_rust_coverage_tool_identity(repo_root)?;
    Ok(TOOLS.get_or_init(|| tools).clone())
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
