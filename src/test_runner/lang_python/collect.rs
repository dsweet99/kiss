use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use kiss::rpytest_runner::{PytestCollectRequest, collect_pytest_nodeids};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct CollectMemoKey {
    repo_root: PathBuf,
    paths: Vec<PathBuf>,
    pytest_args: Vec<String>,
    inventory: String,
}

fn collection_input_stamp(repo_root: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for name in [
        "pytest.ini",
        "pyproject.toml",
        "setup.cfg",
        "tox.ini",
        ".kissconfig",
    ] {
        h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(name.len() as u8);
        if let Ok(bytes) = fs::read(repo_root.join(name)) {
            for byte in bytes {
                h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(byte);
            }
        }
    }
    h = mix_collection_audit(h, repo_root);
    format!("{h:016x}")
}

fn mix_collection_audit(mut h: u64, repo_root: &Path) -> u64 {
    let Ok(dir) =
        crate::test_runner::python_coverage_index::storage::python_coverage_cache_root(repo_root)
    else {
        return h;
    };
    let path = dir.join("collection_audit.json");
    let Ok(bytes) = fs::read(&path) else {
        return h;
    };
    for byte in &bytes {
        h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(*byte);
    }
    if let Ok(listed) = serde_json::from_slice::<Vec<String>>(&bytes) {
        for rel in listed {
            if let Ok(contents) = fs::read(repo_root.join(&rel)) {
                h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(rel.len() as u8);
                for byte in contents {
                    h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(byte);
                }
            }
        }
    }
    h
}

fn persist_collection_audit(repo_root: &Path, observed: &[String]) {
    let Ok(dir) =
        crate::test_runner::python_coverage_index::storage::python_coverage_cache_root(repo_root)
    else {
        return;
    };
    let _ = fs::create_dir_all(&dir);
    let mut listed: Vec<String> = observed
        .iter()
        .filter(|rel| rel.ends_with(".py") && !rel.contains("__pycache__"))
        .cloned()
        .collect();
    listed.sort();
    listed.dedup();
    if let Ok(bytes) = serde_json::to_vec(&listed) {
        let _ = fs::write(dir.join("collection_audit.json"), bytes);
    }
}

static COLLECT_MEMO: Mutex<BTreeMap<CollectMemoKey, Vec<String>>> = Mutex::new(BTreeMap::new());

#[cfg(test)]
static FULL_SUITE_SUBPROCESS_COLLECTS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn collect_python_nodeids(
    repo_root: &Path,
    paths: Option<&[PathBuf]>,
    pytest_args: &[String],
) -> Result<Vec<String>, String> {
    let normalized_paths = paths.map(normalize_collect_paths).unwrap_or_default();
    let key = CollectMemoKey {
        repo_root: repo_root.to_path_buf(),
        paths: normalized_paths.clone(),
        pytest_args: pytest_args.to_vec(),
        inventory: collection_input_stamp(repo_root),
    };
    if let Some(cached) = COLLECT_MEMO.lock().unwrap().get(&key).cloned() {
        return Ok(cached);
    }
    if paths.is_none() {
        #[cfg(test)]
        FULL_SUITE_SUBPROCESS_COLLECTS.fetch_add(1, Ordering::SeqCst);
    }
    let req = PytestCollectRequest {
        cwd: repo_root.to_path_buf(),
        python: PathBuf::from("python"),
        paths: normalized_paths,
        pytest_args: pytest_args.to_vec(),
        env: BTreeMap::new(),
    };
    let outcome = collect_pytest_nodeids(req).map_err(format_collect_error)?;
    persist_collection_audit(repo_root, &outcome.observed_workspace);
    if !outcome.unsupported_external {
        COLLECT_MEMO
            .lock()
            .unwrap()
            .insert(key, outcome.nodeids.clone());
    }
    Ok(outcome.nodeids)
}

fn normalize_collect_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = paths.to_vec();
    out.sort();
    out
}

fn format_collect_error(err: kiss::rpytest_runner::PytestCollectError) -> String {
    match err {
        kiss::rpytest_runner::PytestCollectError::InvalidRequest(message) => {
            format!("error: kiss test: invalid pytest collection request: {message}")
        }
        kiss::rpytest_runner::PytestCollectError::Spawn { program, message } => {
            format!(
                "error: kiss test: failed to spawn {} for pytest collection: {message}",
                program.display()
            )
        }
        kiss::rpytest_runner::PytestCollectError::CollectionFailed {
            exit_code,
            stderr,
            stdout,
        } => {
            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                stdout.trim().to_string()
            };
            format!(
                "error: kiss test: pytest collection failed (exit={:?}): {detail}",
                exit_code
            )
        }
        kiss::rpytest_runner::PytestCollectError::InvalidOutput(message) => {
            format!("error: kiss test: invalid pytest collection output: {message}")
        }
        kiss::rpytest_runner::PytestCollectError::NodeidNormalization { nodeid, message } => {
            format!("error: kiss test: invalid pytest nodeid '{nodeid}': {message}")
        }
    }
}

#[cfg(test)]
pub(crate) fn format_collect_error_for_test(
    err: kiss::rpytest_runner::PytestCollectError,
) -> String {
    format_collect_error(err)
}

pub(crate) fn clear_python_collect_memo() {
    COLLECT_MEMO.lock().unwrap().clear();
    crate::test_runner::lang_python::generation::clear_python_execution_identity_memo();
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn reset_python_collect_memo_for_tests() {
    clear_python_collect_memo();
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn full_suite_subprocess_collects_for_tests() -> usize {
    FULL_SUITE_SUBPROCESS_COLLECTS.load(Ordering::SeqCst)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn reset_full_suite_subprocess_collects_for_tests() {
    FULL_SUITE_SUBPROCESS_COLLECTS.store(0, Ordering::SeqCst);
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use kiss::rpytest_runner::PytestCollectError;

    #[test]
    fn witness_python_collect_helpers() {
        let key = CollectMemoKey {
            repo_root: PathBuf::from("."),
            paths: normalize_collect_paths(&[PathBuf::from("tests/t.py")]),
            pytest_args: vec!["-q".into()],
            inventory: collection_input_stamp(Path::new(".")),
        };
        let _ = key;
        assert!(
            format_collect_error(PytestCollectError::InvalidRequest("x".into()))
                .contains("invalid pytest collection request")
        );
        reset_python_collect_memo_for_tests();
        let before = full_suite_subprocess_collects_for_tests();
        reset_full_suite_subprocess_collects_for_tests();
        assert_eq!(full_suite_subprocess_collects_for_tests(), 0);
        let _ = before;
    }
}
