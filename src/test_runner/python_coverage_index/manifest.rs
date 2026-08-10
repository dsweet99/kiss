use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::storage::{
    normalized_python_repo_root, python_coverage_cache_root, python_entries_fingerprint,
    python_population_manifest_path, python_source_input_fingerprint, python_unique_suffix,
};
use super::{POPULATION_SCHEMA_VERSION, PYTHON_SELECTOR_DISCOVERY_VERSION};

pub(crate) const PYTHON_COVERAGE_ENV_KEYS: &[&str] = &["PYTHONPATH"];

#[cfg(test)]
pub(crate) fn write_python_population_manifest_for_args(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> Result<(), String> {
    let identity = current_python_population_manifest_identity(repo_root, test_args)?;
    write_python_population_manifest_with_identity(repo_root, selectors, &identity)
}

pub(crate) fn python_population_manifest_is_current_for_args_with_env_keys(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
    env_keys: &[&str],
) -> bool {
    let _ = env_keys;
    if super::generation::current_complete_generation_matches(repo_root, selectors, test_args) {
        return true;
    }
    let Ok(identity) =
        current_python_population_manifest_identity_with_env_keys(repo_root, test_args, env_keys)
    else {
        return false;
    };
    // Prefer the warm-seal plan path when available: skip tree rehash / entry restat.
    if python_population_manifest_is_current_for_warm_seal(repo_root, selectors, &identity) {
        return true;
    }
    python_population_manifest_is_current_with_identity(repo_root, selectors, &identity)
}

fn python_population_manifest_is_current_for_warm_seal(
    repo_root: &Path,
    selectors: &[String],
    identity: &PythonPopulationManifestIdentity,
) -> bool {
    let Ok(cache_root) = python_coverage_cache_root(repo_root) else {
        return false;
    };
    if !rslip::warm_hit_seal_exists(&cache_root) {
        return false;
    }
    let Some(manifest) = read_python_population_manifest(repo_root) else {
        return false;
    };
    manifest.matches_python_identity(identity, &normalized_python_repo_root(repo_root))
        && manifest.matches_python_selectors(selectors)
}

#[cfg(test)]
pub(crate) fn write_python_population_manifest_with_identity(
    repo_root: &Path,
    selectors: &[String],
    identity: &PythonPopulationManifestIdentity,
) -> Result<(), String> {
    let entries_fingerprint = python_entries_fingerprint(&python_coverage_cache_root(repo_root)?)
        .map_err(|e| e.to_string())?;
    write_python_population_manifest_with_identity_and_entries_fingerprint(
        repo_root,
        selectors,
        identity,
        &entries_fingerprint,
    )
}

#[allow(dead_code)]
pub(crate) fn write_python_population_manifest_with_identity_and_entries_fingerprint(
    repo_root: &Path,
    selectors: &[String],
    identity: &PythonPopulationManifestIdentity,
    entries_fingerprint: &str,
) -> Result<(), String> {
    let mut selectors = selectors.to_vec();
    selectors.sort();
    selectors.dedup();
    let path = python_population_manifest_path(repo_root)?;
    let parent = path.parent().ok_or_else(|| {
        "error: kiss test: Python population manifest path has no parent".to_string()
    })?;
    let tmp_path = parent.join(format!(".population.{}.tmp", python_unique_suffix()));
    let payload = PythonPopulationManifest {
        schema_version: POPULATION_SCHEMA_VERSION.to_string(),
        cache_schema_version: identity.cache_schema_version.clone(),
        source_root: normalized_python_repo_root(repo_root),
        selector_discovery_version: identity.selector_discovery_version.clone(),
        python_version: identity.python_version.clone(),
        pytest_version: identity.pytest_version.clone(),
        pytest_args: identity.pytest_args.clone(),
        env: identity.env.clone(),
        input_fingerprint: python_source_input_fingerprint(repo_root).map_err(|e| e.to_string())?,
        entries_fingerprint: entries_fingerprint.to_string(),
        selectors,
    };
    kiss_publication_barrier::publish_atomically("python_population", &path, &tmp_path, |file| {
        serde_json::to_writer_pretty(&mut *file, &payload).map_err(std::io::Error::other)?;
        use std::io::Write;
        file.write_all(b"\n")?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

pub(crate) fn python_population_manifest_is_current_with_identity(
    repo_root: &Path,
    selectors: &[String],
    identity: &PythonPopulationManifestIdentity,
) -> bool {
    let Some(manifest) = read_python_population_manifest(repo_root) else {
        return false;
    };
    let Ok(input_fingerprint) = python_source_input_fingerprint(repo_root) else {
        return false;
    };
    let Some(entries_fingerprint) = current_python_entries_fingerprint(repo_root) else {
        return false;
    };
    manifest.matches_python_identity(identity, &normalized_python_repo_root(repo_root))
        && manifest.input_fingerprint == input_fingerprint
        && manifest.entries_fingerprint == entries_fingerprint
        && manifest.matches_python_selectors(selectors)
}

pub(crate) fn read_python_population_manifest(
    repo_root: &Path,
) -> Option<PythonPopulationManifest> {
    fs::read(python_population_manifest_path(repo_root).ok()?)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PythonPopulationManifest>(&bytes).ok())
}

pub(crate) fn stored_python_universe_selectors(
    repo_root: &Path,
    test_args: &[String],
    env_keys: &[&str],
) -> Option<Vec<String>> {
    stored_python_universe_population(repo_root, test_args, env_keys)
        .map(|population| population.selectors)
}

pub(crate) struct StoredPythonPopulation {
    pub(crate) selectors: Vec<String>,
    pub(crate) identity: String,
}

pub(crate) fn stored_python_universe_population(
    repo_root: &Path,
    test_args: &[String],
    env_keys: &[&str],
) -> Option<StoredPythonPopulation> {
    let _ = env_keys;
    if let Ok(pinned) = super::generation::try_load_pinned_python_generation(repo_root) {
        let exec = super::generation::current_python_execution_identity(repo_root, test_args).ok()?;
        if pinned.plan.base_identity == exec && pinned.complete {
            valid_stored_selectors(&pinned.plan.selectors)?;
            return Some(StoredPythonPopulation {
                selectors: pinned.plan.selectors.clone(),
                identity: format!("gen:{}", pinned.generation_id),
            });
        }
        // Incompatible/incomplete generation: fall through to v1 sidecars when present.
    }
    let identity =
        current_python_population_manifest_identity_with_env_keys(repo_root, test_args, env_keys)
            .ok()?;
    let manifest = read_python_population_manifest(repo_root)?;
    let input_fingerprint = python_source_input_fingerprint(repo_root).ok()?;
    let entries_fingerprint = current_python_entries_fingerprint(repo_root)?;
    if manifest.matches_python_identity(&identity, &normalized_python_repo_root(repo_root))
        && manifest.input_fingerprint == input_fingerprint
        && manifest.entries_fingerprint == entries_fingerprint
    {
        valid_stored_selectors(&manifest.selectors)?;
        let identity = stable_population_identity(&manifest);
        Some(StoredPythonPopulation {
            selectors: manifest.selectors.clone(),
            identity,
        })
    } else {
        None
    }
}

pub(crate) fn stored_python_universe_selectors_for_current_inputs(
    repo_root: &Path,
    test_args: &[String],
    env_keys: &[&str],
) -> Option<Vec<String>> {
    if let Ok(pinned) = super::generation::try_load_pinned_python_generation(repo_root) {
        let exec = super::generation::current_python_execution_identity(repo_root, test_args).ok()?;
        if pinned.plan.base_identity == exec {
            valid_stored_selectors(&pinned.plan.selectors)?;
            return Some(pinned.plan.selectors.clone());
        }
        return None;
    }
    let identity =
        current_python_population_manifest_identity_with_env_keys(repo_root, test_args, env_keys)
            .ok()?;
    let manifest = read_python_population_manifest(repo_root)?;
    let input_fingerprint = python_source_input_fingerprint(repo_root).ok()?;
    if manifest.matches_python_identity(&identity, &normalized_python_repo_root(repo_root))
        && manifest.input_fingerprint == input_fingerprint
    {
        valid_stored_selectors(&manifest.selectors)?;
        Some(manifest.selectors.clone())
    } else {
        None
    }
}

pub(crate) fn python_population_environment_mismatch(
    repo_root: &Path,
    test_args: &[String],
    env_keys: &[&str],
) -> Option<(BTreeMap<String, String>, BTreeMap<String, String>)> {
    let identity =
        current_python_population_manifest_identity_with_env_keys(repo_root, test_args, env_keys)
            .ok()?;
    if let Ok(pinned) = super::generation::try_load_pinned_python_generation(repo_root) {
        return (pinned.plan.base_identity.env != identity.env)
            .then_some((pinned.plan.base_identity.env.clone(), identity.env));
    }
    let manifest = read_python_population_manifest(repo_root)?;
    (manifest.env != identity.env).then_some((manifest.env, identity.env))
}

fn valid_stored_selectors(selectors: &[String]) -> Option<()> {
    selectors
        .windows(2)
        .all(|pair| pair[0] < pair[1])
        .then_some(())
}

fn stable_population_identity(manifest: &PythonPopulationManifest) -> String {
    let mut h =
        super::storage::python_fnv1a64(0xcbf2_9ce4_8422_2325, b"kiss-python-runtime-population-v1");
    for value in [
        manifest.schema_version.as_str(),
        manifest.cache_schema_version.as_str(),
        manifest.source_root.as_str(),
        manifest.selector_discovery_version.as_str(),
        manifest.python_version.as_str(),
        manifest.pytest_version.as_str(),
        manifest.input_fingerprint.as_str(),
    ] {
        h = super::storage::python_fnv1a64(h, value.as_bytes());
        h = super::storage::python_fnv1a64(h, &[0]);
    }
    for arg in &manifest.pytest_args {
        h = super::storage::python_fnv1a64(h, arg.as_bytes());
        h = super::storage::python_fnv1a64(h, &[0]);
    }
    for (key, value) in &manifest.env {
        h = super::storage::python_fnv1a64(h, key.as_bytes());
        h = super::storage::python_fnv1a64(h, b"=");
        h = super::storage::python_fnv1a64(h, value.as_bytes());
        h = super::storage::python_fnv1a64(h, &[0]);
    }
    for selector in &manifest.selectors {
        h = super::storage::python_fnv1a64(h, selector.as_bytes());
        h = super::storage::python_fnv1a64(h, &[0]);
    }
    format!("{h:016x}")
}

fn current_python_entries_fingerprint(repo_root: &Path) -> Option<String> {
    let cache_root = python_coverage_cache_root(repo_root).ok()?;
    python_entries_fingerprint(&cache_root).ok()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PythonPopulationManifestIdentity {
    pub(crate) cache_schema_version: String,
    pub(crate) selector_discovery_version: String,
    pub(crate) python_version: String,
    pub(crate) pytest_version: String,
    pub(crate) pytest_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
}

impl PythonPopulationManifestIdentity {
    pub(crate) fn has_python_tool_versions(&self) -> bool {
        !self.python_version.trim().is_empty() && !self.pytest_version.trim().is_empty()
    }
}

pub(crate) fn current_python_population_manifest_identity(
    repo_root: &Path,
    test_args: &[String],
) -> Result<PythonPopulationManifestIdentity, String> {
    current_python_population_manifest_identity_with_env_keys(
        repo_root,
        test_args,
        PYTHON_COVERAGE_ENV_KEYS,
    )
}

fn current_python_population_manifest_identity_with_env_keys(
    repo_root: &Path,
    test_args: &[String],
    env_keys: &[&str],
) -> Result<PythonPopulationManifestIdentity, String> {
    let (python_version, pytest_version) =
        crate::test_runner::runners::detect_rslip_versions(repo_root)?;
    Ok(PythonPopulationManifestIdentity {
        cache_schema_version: rslip::CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: PYTHON_SELECTOR_DISCOVERY_VERSION.to_string(),
        python_version,
        pytest_version,
        pytest_args: test_args.to_vec(),
        // Ignore env_keys contents for PYTHONPATH: always normalize via repo root.
        // Callers still pass PYTHON_COVERAGE_ENV_KEYS for allowlist documentation.
        env: {
            let _ = env_keys;
            kiss::python_coverage_env_map(repo_root)
        },
    })
}

#[cfg(test)]
fn relevant_python_coverage_env(
    repo_root: &Path,
    env_keys: &[&str],
) -> BTreeMap<String, String> {
    let _ = env_keys;
    kiss::python_coverage_env_map(repo_root)
}

#[derive(Deserialize, Serialize)]
pub(crate) struct PythonPopulationManifest {
    pub(crate) schema_version: String,
    pub(crate) cache_schema_version: String,
    pub(crate) source_root: String,
    pub(crate) selector_discovery_version: String,
    pub(crate) python_version: String,
    pub(crate) pytest_version: String,
    pub(crate) pytest_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) input_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) selectors: Vec<String>,
}

impl PythonPopulationManifest {
    pub(crate) fn matches_python_identity(
        &self,
        identity: &PythonPopulationManifestIdentity,
        source_root: &str,
    ) -> bool {
        identity.has_python_tool_versions()
            && self.schema_version == POPULATION_SCHEMA_VERSION
            && self.cache_schema_version == identity.cache_schema_version
            && self.source_root == source_root
            && self.selector_discovery_version == identity.selector_discovery_version
            && self.python_version == identity.python_version
            && self.pytest_version == identity.pytest_version
            && self.pytest_args == identity.pytest_args
            && self.env == identity.env
    }

    pub(crate) fn matches_python_selectors(&self, selectors: &[String]) -> bool {
        if self.selectors.len() == selectors.len()
            && self
                .selectors
                .iter()
                .zip(selectors.iter())
                .all(|(left, right)| left == right)
        {
            return true;
        }
        let mut expected = selectors.to_vec();
        expected.sort();
        expected.dedup();
        self.selectors == expected
    }
}

#[cfg(test)]
#[path = "manifest_test.rs"]
mod manifest_test;
