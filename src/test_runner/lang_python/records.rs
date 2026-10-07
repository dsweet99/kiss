//! Python test records: what each pytest result is stored under and the inputs it
//! depends on. Any change to a Python input reruns every Python test.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kiss::test_records::TestRecord;

use crate::test_runner::lang_iface::records::{
    Outcome, RecordScope, cache_policy, current_deps, declared_inputs_digest, digest,
};

const IDENTITY_SCHEMA: &str = "kiss-python-pytest-record-v1";

/// What every Python record is stored under: the interpreter and pytest versions, the
/// pytest arguments, and the environment. A record made under another identity never holds.
pub(crate) fn record_identity(repo_root: &Path, extras: &[String]) -> Result<String, String> {
    let (python, pytest) = super::versions::detect_python_versions(repo_root)?;
    let payload = serde_json::json!({
        "schema": IDENTITY_SCHEMA,
        "python": python,
        "pytest": pytest,
        "args": extras,
        "env": super::versions::pytest_env(repo_root),
    });
    Ok(format!(
        "python-pytest:{}",
        digest(payload.to_string().as_bytes())
    ))
}

/// One digest of every Python input in the repository: `.py` files and pytest config.
pub(crate) fn python_inputs_digest(repo_root: &Path) -> io::Result<String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut bytes = b"python-workspace-inputs-v2\0".to_vec();
    for path in python_input_paths(&root)? {
        let rel = path.strip_prefix(&root).unwrap_or(&path);
        bytes.extend_from_slice(rel.to_string_lossy().as_bytes());
        bytes.push(0);
        bytes.extend(fs::read(&path)?);
        bytes.push(0);
    }
    Ok(digest(&bytes))
}

fn python_input_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_python_inputs(root, &mut out)?;
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    Ok(out)
}

fn visit_python_inputs(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if !skips_python_input_dir(&path) {
                visit_python_inputs(&path, out)?;
            }
        } else if file_type.is_file() && is_python_input(&path) {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn skips_python_input_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".git" | ".pytest_cache" | "__pycache__" | ".venv" | "venv" | "target" | ".kiss")
    )
}

pub(crate) fn is_python_input(path: &Path) -> bool {
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

/// The time limit, in milliseconds, recorded for `selector`; `None` when the time gate is off.
pub(crate) fn timeout_millis(gate: &kiss::GateConfig, selector: &str) -> Option<u64> {
    if gate.unit_test_time_gate_disabled() {
        return None;
    }
    u64::try_from(super::versions::timeout_for_selector_with_gate(gate, selector).as_millis()).ok()
}

/// Computes the current value of each dependency a Python record keeps.
pub(crate) struct CurrentDeps {
    repo_root: PathBuf,
    gate: kiss::GateConfig,
    inputs: String,
    cache_policy: kiss::test_cache_policy::TestCachePolicy,
}

impl CurrentDeps {
    pub(crate) fn new(repo_root: &Path, gate: &kiss::GateConfig) -> Result<Self, String> {
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            gate: gate.clone(),
            inputs: python_inputs_digest(repo_root)
                .map_err(|err| format!("error: kiss: fingerprint Python sources: {err}"))?,
            cache_policy: cache_policy(repo_root),
        })
    }

    pub(crate) fn of(&self, row: &TestRecord) -> std::collections::BTreeMap<String, String> {
        let declared = declared_inputs_digest(&self.repo_root, &self.cache_policy, &row.test_id);
        current_deps(&self.inputs, declared, row, || {
            timeout_millis(&self.gate, &row.test_id)
        })
    }
}

/// Stores Python test results under one identity and one inputs digest.
pub(crate) struct RecordWriter {
    repo_root: PathBuf,
    identity: String,
    inputs: String,
    cache_policy: kiss::test_cache_policy::TestCachePolicy,
}

impl RecordWriter {
    pub(crate) fn new(repo_root: &Path, identity: String) -> Result<Self, String> {
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            identity,
            inputs: python_inputs_digest(repo_root)
                .map_err(|err| format!("error: kiss: fingerprint Python sources: {err}"))?,
            cache_policy: cache_policy(repo_root),
        })
    }

    pub(crate) fn store(
        &self,
        test_id: &str,
        status: kiss::rpytest_runner::TestStatus,
        duration: std::time::Duration,
        gate: &kiss::GateConfig,
    ) -> Result<(), String> {
        crate::test_runner::lang_iface::records::store(
            RecordScope {
                repo_root: &self.repo_root,
                language: "python",
                identity: &self.identity,
            },
            &self.inputs,
            &Outcome {
                test_id,
                status,
                duration,
                timeout_ms: timeout_millis(gate, test_id),
                declared_inputs: declared_inputs_digest(
                    &self.repo_root,
                    &self.cache_policy,
                    test_id,
                ),
            },
        )
    }
}

/// Stores a passing record for each selector under the repo's current identity and inputs.
#[cfg(test)]
pub(crate) fn store_records(
    repo_root: &Path,
    outcomes: &[(&str, kiss::rpytest_runner::TestStatus)],
) {
    let identity = record_identity(repo_root, &[]).expect("python record identity");
    let writer = RecordWriter::new(repo_root, identity).expect("python record writer");
    let gate = kiss::GateConfig::default();
    for (selector, status) in outcomes {
        writer
            .store(
                selector,
                *status,
                std::time::Duration::from_millis(1),
                &gate,
            )
            .expect("store python record");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_digest_changes_with_python_sources_and_config() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("app.py"), "x = 1\n").unwrap();
        let before = python_inputs_digest(root).unwrap();
        assert_eq!(before, python_inputs_digest(root).unwrap());
        fs::write(root.join("app.py"), "x = 2\n").unwrap();
        let edited = python_inputs_digest(root).unwrap();
        assert_ne!(before, edited);
        fs::write(root.join("pytest.ini"), "[pytest]\n").unwrap();
        assert_ne!(edited, python_inputs_digest(root).unwrap());
    }

    #[test]
    fn inputs_digest_ignores_caches_and_non_python_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("app.py"), "x = 1\n").unwrap();
        let before = python_inputs_digest(root).unwrap();
        fs::create_dir_all(root.join("__pycache__")).unwrap();
        fs::write(root.join("__pycache__/app.py"), "stale\n").unwrap();
        fs::create_dir_all(root.join(".kiss")).unwrap();
        fs::write(root.join(".kiss/state.py"), "x\n").unwrap();
        fs::write(root.join("notes.txt"), "x\n").unwrap();
        assert_eq!(before, python_inputs_digest(root).unwrap());
    }

    #[test]
    fn timeouts_follow_the_gate() {
        let mut gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 2.5)],
            ..Default::default()
        };
        assert_eq!(timeout_millis(&gate, "tests/test_a.py::t"), Some(2500));
        gate.max_unit_test_seconds.clear();
        assert_eq!(timeout_millis(&gate, "tests/test_a.py::t"), None);
    }
}
