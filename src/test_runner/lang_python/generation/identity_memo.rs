//! Cycle-scoped memo for `current_python_execution_identity`.
//!
//! Plan, ensure, and cov must share one identity per watch/test cycle so the
//! Python tree is not walked separately with divergent results.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::types::PythonExecutionIdentity;

static IDENTITY_MEMO: Mutex<Option<IdentityMemoEntry>> = Mutex::new(None);

struct IdentityMemoEntry {
    repo_root: PathBuf,
    test_args: Vec<String>,
    identity: PythonExecutionIdentity,
}

/// Clear the execution-identity memo. Call at the start of each test/watch cycle.
pub(crate) fn clear_python_execution_identity_memo() {
    if let Ok(mut guard) = IDENTITY_MEMO.lock() {
        *guard = None;
    }
}

pub(crate) fn memoized_or_compute_identity(
    repo_root: &Path,
    test_args: &[String],
    compute: impl FnOnce() -> Result<PythonExecutionIdentity, String>,
) -> Result<PythonExecutionIdentity, String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    if let Ok(guard) = IDENTITY_MEMO.lock()
        && let Some(entry) = guard.as_ref()
        && entry.repo_root == root
        && entry.test_args == test_args
    {
        return Ok(entry.identity.clone());
    }
    let identity = compute()?;
    if let Ok(mut guard) = IDENTITY_MEMO.lock() {
        *guard = Some(IdentityMemoEntry {
            repo_root: root,
            test_args: test_args.to_vec(),
            identity: identity.clone(),
        });
    }
    Ok(identity)
}
