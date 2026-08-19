
use std::path::Path;

use super::identity::{current_python_execution_identity, identity_matches_current};
use super::load::{GenerationLoadError, try_load_pinned_python_generation_warm};
use super::types::PinnedPythonGeneration;

pub(crate) fn current_complete_generation_matches(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> bool {
    match try_load_pinned_python_generation_warm(repo_root) {
        Ok(pinned) => generation_matches(&pinned, repo_root, selectors, test_args) && pinned.complete,
        Err(_) => false,
    }
}

pub(crate) fn current_generation_plan_matches(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> bool {
    match try_load_pinned_python_generation_warm(repo_root) {
        Ok(pinned) => generation_matches(&pinned, repo_root, selectors, test_args),
        Err(_) => false,
    }
}

#[allow(dead_code)]
pub(crate) fn current_generation_matches_plan(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> Option<PinnedPythonGeneration> {
    let pinned = try_load_pinned_python_generation_warm(repo_root).ok()?;
    generation_matches(&pinned, repo_root, selectors, test_args).then_some(pinned)
}

pub(crate) fn generation_matches(
    pinned: &PinnedPythonGeneration,
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> bool {
    if !identity_matches_current(repo_root, &pinned.plan.base_identity, test_args) {
        return false;
    }


    let mut expected = selectors.to_vec();
    expected.sort();
    expected.dedup();
    pinned.plan.selectors == expected
}

#[allow(dead_code)]
pub(crate) fn load_generation_or_stale(
    repo_root: &Path,
) -> Result<PinnedPythonGeneration, GenerationLoadError> {
    try_load_pinned_python_generation_warm(repo_root)
}

#[allow(dead_code)]
pub(crate) fn current_identity_fingerprint(
    repo_root: &Path,
    test_args: &[String],
) -> Option<String> {
    let identity = current_python_execution_identity(repo_root, test_args).ok()?;
    Some(format!(
        "{}:{}:{}",
        identity.input_fingerprint, identity.python_version, identity.pytest_version
    ))
}
