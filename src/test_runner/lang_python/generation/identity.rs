//! Build PythonExecutionIdentity / PythonPopulationPlan.

use std::path::Path;

use crate::test_runner::python_coverage_index::manifest::{
    PYTHON_COVERAGE_ENV_KEYS, PythonPopulationManifestIdentity,
    current_python_population_manifest_identity,
};
use crate::test_runner::python_coverage_index::storage::{
    normalized_python_repo_root, python_fnv1a64, python_source_input_fingerprint,
};
use crate::test_runner::python_coverage_index::PYTHON_SELECTOR_DISCOVERY_VERSION;
use super::types::{
    COLLECTOR_SEMANTICS_VERSION, GENERATION_SCHEMA_VERSION, PythonExecutionIdentity,
    PythonPopulationPlan, RUNNER_SEMANTICS_VERSION,
};

pub(crate) fn current_python_execution_identity(
    repo_root: &Path,
    test_args: &[String],
) -> Result<PythonExecutionIdentity, String> {
    super::identity_memo::memoized_or_compute_identity(repo_root, test_args, || {
        let base = current_python_population_manifest_identity(repo_root, test_args)?;
        identity_from_manifest_identity(repo_root, &base)
    })
}

pub(crate) fn identity_from_manifest_identity(
    repo_root: &Path,
    base: &PythonPopulationManifestIdentity,
) -> Result<PythonExecutionIdentity, String> {
    let input_fingerprint =
        python_source_input_fingerprint(repo_root).map_err(|e| e.to_string())?;
    let env = kiss::python_coverage_env_map(repo_root);
    Ok(PythonExecutionIdentity {
        schema_version: GENERATION_SCHEMA_VERSION.to_string(),
        runner_semantics_version: RUNNER_SEMANTICS_VERSION.to_string(),
        collector_semantics_version: COLLECTOR_SEMANTICS_VERSION.to_string(),
        source_root: normalized_python_repo_root(repo_root),
        interpreter_identity: base.python_version.clone(),
        python_version: base.python_version.clone(),
        pytest_version: base.pytest_version.clone(),
        plugin_identities: plugin_identities_from_args(&base.pytest_args),
        pytest_args: base.pytest_args.clone(),
        pytest_config_digest: digest_string_list(&base.pytest_args),
        kissconfig_test_digest: kissconfig_test_digest(repo_root),
        coverage_env_digest: digest_env_map(&env),
        env,
        input_fingerprint,
        selector_discovery_version: PYTHON_SELECTOR_DISCOVERY_VERSION.to_string(),
        cache_schema_version: base.cache_schema_version.clone(),
    })
}

pub(crate) fn population_plan_for_selectors(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> Result<PythonPopulationPlan, String> {
    let mut selectors = selectors.to_vec();
    selectors.sort();
    selectors.dedup();
    Ok(PythonPopulationPlan {
        base_identity: current_python_execution_identity(repo_root, test_args)?,
        selectors,
    })
}

pub(crate) fn identity_matches_current(
    repo_root: &Path,
    identity: &PythonExecutionIdentity,
    test_args: &[String],
) -> bool {
    // Full equality including input_fingerprint. Planning, test ensure, and
    // coverage load share this predicate so a fingerprint change cannot
    // warm-accept a generation that cov would reject.
    let Ok(current) = current_python_execution_identity(repo_root, test_args) else {
        return false;
    };
    identity == &current
}

fn plugin_identities_from_args(args: &[String]) -> Vec<String> {
    let mut plugins = Vec::new();
    let mut idx = 0;
    while idx + 1 < args.len() {
        if args[idx] == "-p" {
            plugins.push(args[idx + 1].clone());
            idx += 2;
            continue;
        }
        idx += 1;
    }
    plugins.sort();
    plugins.dedup();
    plugins
}

fn kissconfig_test_digest(_repo_root: &Path) -> String {
    let cfg = kiss::TestSectionConfig::load();
    let mut h = python_fnv1a64(0xcbf2_9ce4_8422_2325, b"kissconfig-test-v1");
    h = python_fnv1a64(h, &cfg.num_jobs.to_le_bytes());
    for plugin in &cfg.pytest_plugins {
        h = python_fnv1a64(h, plugin.as_bytes());
        h = python_fnv1a64(h, &[0]);
    }
    for ignore in &cfg.ignore {
        h = python_fnv1a64(h, ignore.as_bytes());
        h = python_fnv1a64(h, &[0]);
    }
    format!("{h:016x}")
}

fn digest_string_list(values: &[String]) -> String {
    let mut h = python_fnv1a64(0xcbf2_9ce4_8422_2325, b"python-string-list-v1");
    for value in values {
        h = python_fnv1a64(h, value.as_bytes());
        h = python_fnv1a64(h, &[0]);
    }
    format!("{h:016x}")
}

fn digest_env_map(env: &std::collections::BTreeMap<String, String>) -> String {
    let mut h = python_fnv1a64(0xcbf2_9ce4_8422_2325, b"python-cov-env-v1");
    for (key, value) in env {
        h = python_fnv1a64(h, key.as_bytes());
        h = python_fnv1a64(h, b"=");
        h = python_fnv1a64(h, value.as_bytes());
        h = python_fnv1a64(h, &[0]);
    }
    let _ = PYTHON_COVERAGE_ENV_KEYS;
    format!("{h:016x}")
}
