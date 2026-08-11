use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use rpytest_runner::{PytestCollectRequest, collect_pytest_nodeids};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct CollectMemoKey {
    repo_root: PathBuf,
    paths: Vec<PathBuf>,
    pytest_args: Vec<String>,
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
    COLLECT_MEMO
        .lock()
        .unwrap()
        .insert(key, outcome.nodeids.clone());
    Ok(outcome.nodeids)
}

fn normalize_collect_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = paths.to_vec();
    out.sort();
    out
}

fn format_collect_error(err: rpytest_runner::PytestCollectError) -> String {
    match err {
        rpytest_runner::PytestCollectError::InvalidRequest(message) => {
            format!("error: kiss test: invalid pytest collection request: {message}")
        }
        rpytest_runner::PytestCollectError::Spawn { program, message } => {
            format!(
                "error: kiss test: failed to spawn {} for pytest collection: {message}",
                program.display()
            )
        }
        rpytest_runner::PytestCollectError::CollectionFailed {
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
        rpytest_runner::PytestCollectError::InvalidOutput(message) => {
            format!("error: kiss test: invalid pytest collection output: {message}")
        }
        rpytest_runner::PytestCollectError::NodeidNormalization { nodeid, message } => {
            format!("error: kiss test: invalid pytest nodeid '{nodeid}': {message}")
        }
    }
}

#[cfg(test)]
pub(crate) fn format_collect_error_for_test(err: rpytest_runner::PytestCollectError) -> String {
    format_collect_error(err)
}

pub(crate) fn clear_python_collect_memo() {
    COLLECT_MEMO.lock().unwrap().clear();
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
    use rpytest_runner::PytestCollectError;

    #[test]
    fn witness_python_collect_helpers() {
        let key = CollectMemoKey {
            repo_root: PathBuf::from("."),
            paths: normalize_collect_paths(&[PathBuf::from("tests/t.py")]),
            pytest_args: vec!["-q".into()],
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
