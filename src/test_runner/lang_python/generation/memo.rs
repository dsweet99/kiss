//! Command-scoped memo for warm generation loads (one validation per cov command).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::load::{GenerationLoadError, load_pinned_warm_locked};
use crate::test_runner::python_coverage_index::storage::python_coverage_cache_root;
use super::types::PinnedPythonGeneration;

static WARM_MEMO: Mutex<Option<WarmMemoEntry>> = Mutex::new(None);

struct WarmMemoEntry {
    cache_root: PathBuf,
    result: Result<PinnedPythonGeneration, GenerationLoadError>,
}

/// Clear the warm-generation memo. Call at the start of each `kiss cov` command.
pub(crate) fn clear_python_generation_warm_memo() {
    if let Ok(mut guard) = WARM_MEMO.lock() {
        *guard = None;
    }
    super::durations_load::clear_generation_durations_memo();
    super::identity_memo::clear_python_execution_identity_memo();
}

pub(crate) fn try_load_pinned_python_generation_warm_memoized(
    repo_root: &Path,
) -> Result<PinnedPythonGeneration, GenerationLoadError> {
    let cache_root =
        python_coverage_cache_root(repo_root).map_err(GenerationLoadError::Corrupt)?;
    if let Ok(guard) = WARM_MEMO.lock()
        && let Some(entry) = guard.as_ref()
        && entry.cache_root == cache_root
    {
        return entry.result.clone();
    }
    let loaded = {
        let _guard = rslip::lock_rslip_derived_state(&cache_root)
            .map_err(|e| GenerationLoadError::Corrupt(e.to_string()))?;
        load_pinned_warm_locked(&cache_root)
    };
    if let Ok(mut guard) = WARM_MEMO.lock() {
        *guard = Some(WarmMemoEntry {
            cache_root,
            result: loaded.clone(),
        });
    }
    loaded
}
