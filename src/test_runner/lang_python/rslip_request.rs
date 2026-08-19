use std::path::{Path, PathBuf};
use std::time::Duration;

use rslip::RslipRequest;

use crate::test_runner::runners::command_stdout;
use crate::test_runner::python_coverage_index::python_coverage_cache_root;

/// Default per-test ceiling for python population/selective runs.
/// Large enough for sameq slow tests (~137s observed), short enough to stop
/// hung webtester/network tests from blocking `kiss test .` for hours.
pub(crate) const DEFAULT_PYTEST_TIMEOUT: Duration = Duration::from_secs(180);

pub(crate) fn rslip_request_from_parts(
    repo_root: &Path,
    selector: &str,
    extra: &[String],
    python_version: &str,
    pytest_version: &str,
    force_rerun: bool,
    gate: &kiss::GateConfig,
) -> Result<RslipRequest, String> {
    if !python_version_supports_rslip(python_version) {
        return Err(format!(
            "error: kiss test: rslip requires Python 3.12+, found {python_version}"
        ));
    }
    let repo_root = repo_root.canonicalize().map_err(|err| {
        format!(
            "error: kiss test: failed to canonicalize repository root {}: {err}",
            repo_root.display()
        )
    })?;
    Ok(RslipRequest {
        nodeid: selector.to_string(),
        cwd: repo_root.clone(),
        source_root: repo_root.clone(),
        python: PathBuf::from("python"),
        python_version: python_version.to_string(),
        pytest_version: pytest_version.to_string(),
        pytest_args: extra.to_vec(),
        env: kiss::python_coverage_env_map(&repo_root),
        cache_root: python_coverage_cache_root(&repo_root)?,
        force_rerun,
        timeout: Some(timeout_for_selector_with_gate(gate, selector)),
        content_fingerprint: None,
    })
}

/// Test/helper seam: load cwd gate once. Prefer `timeout_for_selector_with_gate` on session paths.
#[cfg(test)]
pub(crate) fn timeout_for_selector(selector: &str) -> Duration {
    timeout_for_selector_with_gate(&kiss::GateConfig::load(), selector)
}

pub(crate) fn timeout_for_selector_with_gate(gate: &kiss::GateConfig, selector: &str) -> Duration {
    if gate.unit_test_time_gate_disabled() {
        return DEFAULT_PYTEST_TIMEOUT;
    }
    let limit = gate.unit_test_seconds_limit(selector);
    if limit <= 0.0 {


        return Duration::ZERO;
    }


    DEFAULT_PYTEST_TIMEOUT
}

pub(crate) fn detect_rslip_versions(repo_root: &Path) -> Result<(String, String), String> {
    if let Some(cached) = read_cached_python_tool_versions(repo_root) {
        return Ok(cached);
    }
    let python = PathBuf::from("python");
    let python_version = command_stdout(
        &python,
        &[
            "-c",
            "import sys; print('.'.join(map(str, sys.version_info[:3])))",
        ],
        repo_root,
    )?;
    let pytest_version = command_stdout(
        &python,
        &["-c", "import pytest; print(pytest.__version__)"],
        repo_root,
    )?;
    let _ = write_cached_python_tool_versions(repo_root, &python_version, &pytest_version);
    Ok((python_version, pytest_version))
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PythonToolVersionsCache {
    python: String,
    pytest: String,
}

fn python_tool_versions_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("python_tool_versions.json")
}

fn read_cached_python_tool_versions(repo_root: &Path) -> Option<(String, String)> {
    let bytes = std::fs::read(python_tool_versions_cache_path(repo_root)).ok()?;
    let cached: PythonToolVersionsCache = serde_json::from_slice(&bytes).ok()?;
    Some((cached.python, cached.pytest))
}

fn write_cached_python_tool_versions(
    repo_root: &Path,
    python: &str,
    pytest: &str,
) -> std::io::Result<()> {
    let path = python_tool_versions_cache_path(repo_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cached = PythonToolVersionsCache {
        python: python.to_string(),
        pytest: pytest.to_string(),
    };
    let bytes = serde_json::to_vec(&cached).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

pub(crate) fn python_version_supports_rslip(version: &str) -> bool {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    matches!((major, minor), (Some(major), Some(minor)) if major > 3 || (major == 3 && minor >= 12))
}

