use std::path::{Path, PathBuf};
use std::time::Duration;

use rslip::RslipRequest;

use crate::test_runner::runners::command_stdout;
use crate::test_runner::python_coverage_index::python_coverage_cache_root;

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

#[cfg(test)]
pub(crate) fn timeout_for_selector(selector: &str) -> Duration {
    timeout_for_selector_with_gate(&kiss::GateConfig::load(), selector)
}

pub(crate) fn timeout_for_selector_with_gate(gate: &kiss::GateConfig, selector: &str) -> Duration {
    if gate.unit_test_time_gate_disabled() {
        return DEFAULT_PYTEST_TIMEOUT;
    }
    duration_from_unit_test_limit(gate.unit_test_seconds_limit(selector))
}

fn duration_from_unit_test_limit(limit: f64) -> Duration {
    if !limit.is_finite() || limit <= 0.0 {
        return Duration::ZERO;
    }
    Duration::from_millis((limit * 1000.0).round().clamp(1.0, u64::MAX as f64) as u64)
}

pub(crate) fn detect_rslip_versions(repo_root: &Path) -> Result<(String, String), String> {
    if let Some(cached) = read_cached_python_tool_versions(repo_root) {
        return Ok(cached);
    }
    let detected = detect_python_tool_versions(repo_root)?;
    let _ = write_cached_python_tool_versions(repo_root, &detected);
    Ok((detected.python, detected.pytest))
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PythonToolVersionsCache {
    python: String,
    pytest: String,
    python_exe: String,
    python_mtime_nanos: u64,
    python_len: u64,
    pytest_file: String,
    pytest_mtime_nanos: u64,
    pytest_len: u64,
    path_python: String,
    path_python_mtime_nanos: u64,
    path_python_len: u64,
}

fn python_tool_versions_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("python_tool_versions.json")
}

fn file_stamp(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)?;
    Some((mtime, meta.len()))
}

fn python_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join("python");
        candidate.is_file().then_some(candidate)
    })
}

fn path_python_stamp() -> Option<(String, u64, u64)> {
    let path = python_on_path()?;
    let (mtime, len) = file_stamp(&path)?;
    Some((path.to_string_lossy().into_owned(), mtime, len))
}

fn stamps_match(path: &str, mtime: u64, len: u64) -> bool {
    file_stamp(Path::new(path)).is_some_and(|(got_mtime, got_len)| got_mtime == mtime && got_len == len)
}

fn read_cached_python_tool_versions(repo_root: &Path) -> Option<(String, String)> {
    let bytes = std::fs::read(python_tool_versions_cache_path(repo_root)).ok()?;
    let cached: PythonToolVersionsCache = serde_json::from_slice(&bytes).ok()?;
    let (path_python, path_mtime, path_len) = path_python_stamp()?;
    if cached.path_python != path_python
        || cached.path_python_mtime_nanos != path_mtime
        || cached.path_python_len != path_len
    {
        return None;
    }
    if !stamps_match(&cached.python_exe, cached.python_mtime_nanos, cached.python_len) {
        return None;
    }
    if !stamps_match(
        &cached.pytest_file,
        cached.pytest_mtime_nanos,
        cached.pytest_len,
    ) {
        return None;
    }
    Some((cached.python, cached.pytest))
}

fn detect_python_tool_versions(repo_root: &Path) -> Result<PythonToolVersionsCache, String> {
    let python = PathBuf::from("python");
    let raw = command_stdout(
        &python,
        &[
            "-c",
            "import sys, pytest\nprint('.'.join(map(str, sys.version_info[:3])))\nprint(pytest.__version__)\nprint(sys.executable)\nprint(pytest.__file__)\n",
        ],
        repo_root,
    )?;
    let mut lines = raw.lines();
    let python_version = next_probe_line(&mut lines, "python version")?;
    let pytest_version = next_probe_line(&mut lines, "pytest version")?;
    let python_exe = next_probe_line(&mut lines, "python executable")?;
    let pytest_file = next_probe_line(&mut lines, "pytest path")?;
    let (python_mtime_nanos, python_len) = file_stamp(Path::new(&python_exe)).ok_or_else(|| {
        format!("error: kiss test: cannot stat python executable {python_exe}")
    })?;
    let (pytest_mtime_nanos, pytest_len) = file_stamp(Path::new(&pytest_file)).ok_or_else(|| {
        format!("error: kiss test: cannot stat pytest at {pytest_file}")
    })?;
    let (path_python, path_python_mtime_nanos, path_python_len) = path_python_stamp()
        .ok_or_else(|| "error: kiss test: python is not on PATH".to_string())?;
    Ok(PythonToolVersionsCache {
        python: python_version,
        pytest: pytest_version,
        python_exe,
        python_mtime_nanos,
        python_len,
        pytest_file,
        pytest_mtime_nanos,
        pytest_len,
        path_python,
        path_python_mtime_nanos,
        path_python_len,
    })
}

fn next_probe_line<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    what: &str,
) -> Result<String, String> {
    lines
        .next()
        .map(str::to_string)
        .ok_or_else(|| format!("error: kiss test: {what} probe produced no output"))
}

fn write_cached_python_tool_versions(
    repo_root: &Path,
    cached: &PythonToolVersionsCache,
) -> std::io::Result<()> {
    let path = python_tool_versions_cache_path(repo_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(cached).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

pub(crate) fn python_version_supports_rslip(version: &str) -> bool {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    matches!((major, minor), (Some(major), Some(minor)) if major > 3 || (major == 3 && minor >= 12))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_python_tool_version_cache_is_redetected() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".kiss")).unwrap();
        std::fs::write(
            tmp.path().join(".kiss").join("python_tool_versions.json"),
            r#"{"python":"0.0.0","pytest":"0.0.0","python_exe":"/nope","python_mtime_nanos":1,"python_len":1,"pytest_file":"/nope","pytest_mtime_nanos":1,"pytest_len":1,"path_python":"/nope","path_python_mtime_nanos":1,"path_python_len":1}"#,
        )
        .unwrap();
        let Ok((py, _)) = detect_rslip_versions(tmp.path()) else {
            return;
        };
        assert_ne!(py, "0.0.0");
    }
}

