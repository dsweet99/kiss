use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use rpytest_runner::TestStatus;
use serde::{Deserialize, Serialize};

use crate::{CACHE_SCHEMA_VERSION, LineCoverage, RslipOutcome, RslipRequest};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RslipCacheEntry {
    schema_version: String,
    pub(crate) nodeid: String,
    pub(crate) status: TestStatus,
    pub(crate) exit_code: Option<i32>,
    pub(crate) duration: std::time::Duration,
    pub(crate) coverage: LineCoverage,
}

impl From<&RslipOutcome> for RslipCacheEntry {
    fn from(outcome: &RslipOutcome) -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION.to_string(),
            nodeid: outcome.nodeid.clone(),
            status: outcome.status,
            exit_code: outcome.exit_code,
            duration: outcome.duration,
            coverage: outcome.coverage.clone(),
        }
    }
}

pub(crate) fn load_rslip_cache_entry(
    cache_root: &Path,
    fingerprint: &str,
) -> Option<RslipCacheEntry> {
    let path = rslip_cache_entry_path(cache_root, fingerprint);
    let bytes = fs::read(path).ok()?;
    let entry: RslipCacheEntry = serde_json::from_slice(&bytes).ok()?;
    (entry.schema_version == CACHE_SCHEMA_VERSION).then_some(entry)
}

pub(crate) fn store_rslip_cache_entry(
    cache_root: &Path,
    fingerprint: &str,
    entry: &RslipCacheEntry,
) -> io::Result<()> {
    let path = rslip_cache_entry_path(cache_root, fingerprint);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp_path = parent.join(format!(".{}.{}.tmp", fingerprint, rslip_unique_suffix()));
    let mut file = create_new_rslip_cache_file(&tmp_path)?;
    serde_json::to_writer(&mut file, entry).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp_path, path)
}

pub(crate) fn create_new_rslip_cache_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

pub(crate) fn rslip_cache_entry_path(cache_root: &Path, fingerprint: &str) -> PathBuf {
    cache_root
        .join("entries")
        .join(format!("{fingerprint}.json"))
}

pub(crate) fn rslip_unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}.{}", process::id(), nanos)
}

pub(crate) fn rslip_cache_fingerprint(req: &RslipRequest) -> io::Result<String> {
    let context = rslip_request_context_fingerprint(req)?;
    Ok(rslip_cache_fingerprint_from_context(&context, &req.nodeid))
}

pub(crate) fn rslip_request_context_fingerprint(req: &RslipRequest) -> io::Result<String> {
    let mut h = rslip_fnv1a64(0xcbf2_9ce4_8422_2325, CACHE_SCHEMA_VERSION.as_bytes());
    h = rslip_fnv1a64(h, req.python.to_string_lossy().as_bytes());
    h = rslip_fnv1a64(h, req.python_version.as_bytes());
    h = rslip_fnv1a64(h, req.pytest_version.as_bytes());
    h = rslip_fnv1a64(h, req.cwd.to_string_lossy().as_bytes());
    h = rslip_fnv1a64(h, req.source_root.to_string_lossy().as_bytes());
    for arg in &req.pytest_args {
        h = rslip_fnv1a64(h, arg.as_bytes());
        h = rslip_fnv1a64(h, &[0]);
    }
    for (key, value) in &req.env {
        h = rslip_fnv1a64(h, key.as_bytes());
        h = rslip_fnv1a64(h, b"=");
        h = rslip_fnv1a64(h, value.as_bytes());
        h = rslip_fnv1a64(h, &[0]);
    }
    for file in rslip_input_files(&req.cwd)? {
        h = rslip_fnv1a64(h, file.to_string_lossy().as_bytes());
        h = rslip_fnv1a64(h, &[0]);
        h = rslip_fnv1a64(h, &fs::read(file)?);
        h = rslip_fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

pub(crate) fn rslip_cache_fingerprint_from_context(
    context_fingerprint: &str,
    nodeid: &str,
) -> String {
    let mut h = rslip_fnv1a64(0xcbf2_9ce4_8422_2325, CACHE_SCHEMA_VERSION.as_bytes());
    h = rslip_fnv1a64(h, context_fingerprint.as_bytes());
    h = rslip_fnv1a64(h, &[0]);
    h = rslip_fnv1a64(h, nodeid.as_bytes());
    format!("{h:016x}")
}

pub(crate) fn rslip_input_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_rslip_inputs(root, &mut out)?;
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    Ok(out)
}

fn visit_rslip_inputs(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_skip_rslip_dir(&path) {
                continue;
            }
            visit_rslip_inputs(&path, out)?;
        } else if file_type.is_file() && is_rslip_cache_input(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn should_skip_rslip_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            ".git" | ".pytest_cache" | "__pycache__" | ".venv" | "venv" | "target" | ".rslip_cache"
        )
    ) || is_kiss_rslip_cache_dir(path)
}

fn is_kiss_rslip_cache_dir(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("rslip_cache")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some(".kiss")
}

fn is_rslip_cache_input(path: &Path) -> bool {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        return true;
    }
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("pytest.ini" | "pyproject.toml" | "setup.cfg" | "tox.ini")
    )
}

pub(crate) fn rslip_fnv1a64(h: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0100_0000_01b3;
    bytes
        .iter()
        .fold(h, |acc, byte| (acc ^ u64::from(*byte)).wrapping_mul(PRIME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn rslip_cache_fingerprint_changes_when_python_content_changes() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("pkg.py"), "def value():\n    return 1\n").unwrap();
        let req = crate::rslip_sample_request(tmp.path());
        let first = rslip_cache_fingerprint(&req).unwrap();
        fs::write(tmp.path().join("pkg.py"), "def value():\n    return 2\n").unwrap();
        let second = rslip_cache_fingerprint(&req).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn rslip_cache_fingerprint_changes_when_python_version_changes() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("pkg.py"), "def value():\n    return 1\n").unwrap();
        let mut req = crate::rslip_sample_request(tmp.path());
        let first = rslip_cache_fingerprint(&req).unwrap();
        req.python_version = "3.13.0".to_string();
        let second = rslip_cache_fingerprint(&req).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn conservative_inputs_include_pytest_config_and_skip_cache_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join(".rslip_cache")).unwrap();
        fs::create_dir(tmp.path().join(".kiss")).unwrap();
        fs::create_dir(tmp.path().join(".kiss").join("rslip_cache")).unwrap();
        fs::write(tmp.path().join("pytest.ini"), "[pytest]\n").unwrap();
        fs::write(tmp.path().join("a.py"), "x = 1\n").unwrap();
        fs::write(
            tmp.path().join(".rslip_cache").join("ignored.py"),
            "x = 2\n",
        )
        .unwrap();
        fs::write(
            tmp.path()
                .join(".kiss")
                .join("rslip_cache")
                .join("ignored.py"),
            "x = 3\n",
        )
        .unwrap();

        let names: BTreeSet<_> = rslip_input_files(tmp.path())
            .unwrap()
            .into_iter()
            .map(|path| path.strip_prefix(tmp.path()).unwrap().to_path_buf())
            .collect();

        assert!(names.contains(Path::new("a.py")));
        assert!(names.contains(Path::new("pytest.ini")));
        assert!(!names.contains(Path::new(".rslip_cache/ignored.py")));
        assert!(!names.contains(Path::new(".kiss/rslip_cache/ignored.py")));
    }

    #[test]
    fn helper_hash_and_temp_suffix_are_usable() {
        assert_ne!(rslip_unique_suffix(), "");
        assert_eq!(
            rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b""),
            0xcbf2_9ce4_8422_2325
        );
        assert_eq!(
            rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"hello"),
            0xa430_d846_80aa_bd0b
        );
        assert_ne!(
            rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"a"),
            rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"b")
        );
    }
}
