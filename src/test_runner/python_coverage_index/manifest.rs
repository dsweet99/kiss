use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::storage::{
    create_new_python_file, normalized_python_repo_root, python_coverage_cache_root,
    python_entries_fingerprint, python_population_manifest_path, python_source_input_fingerprint,
    python_unique_suffix,
};
use super::{POPULATION_SCHEMA_VERSION, PYTHON_SELECTOR_DISCOVERY_VERSION};

pub(crate) const PYTHON_COVERAGE_ENV_KEYS: &[&str] = &["PYTHONPATH"];

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
    let Ok(identity) =
        current_python_population_manifest_identity_with_env_keys(repo_root, test_args, env_keys)
    else {
        return false;
    };
    python_population_manifest_is_current_with_identity(repo_root, selectors, &identity)
}

pub(crate) fn write_python_population_manifest_with_identity(
    repo_root: &Path,
    selectors: &[String],
    identity: &PythonPopulationManifestIdentity,
) -> Result<(), String> {
    let mut selectors = selectors.to_vec();
    selectors.sort();
    selectors.dedup();
    let path = python_population_manifest_path(repo_root);
    let parent = path.parent().ok_or_else(|| {
        "error: kiss test: Python population manifest path has no parent".to_string()
    })?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp_path = parent.join(format!(".population.{}.tmp", python_unique_suffix()));
    let mut file = create_new_python_file(&tmp_path).map_err(|e| e.to_string())?;
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
        entries_fingerprint: python_entries_fingerprint(&python_coverage_cache_root(repo_root))
            .map_err(|e| e.to_string())?,
        selectors,
    };
    serde_json::to_writer_pretty(&mut file, &payload).map_err(|e| e.to_string())?;
    use std::io::Write;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(tmp_path, path).map_err(|e| e.to_string())
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
    manifest.matches_python_identity(identity, &normalized_python_repo_root(repo_root))
        && manifest.input_fingerprint == input_fingerprint
        && manifest.matches_python_selectors(selectors)
}

pub(crate) fn read_python_population_manifest(
    repo_root: &Path,
) -> Option<PythonPopulationManifest> {
    fs::read(python_population_manifest_path(repo_root))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PythonPopulationManifest>(&bytes).ok())
}

pub(crate) fn stored_python_universe_selectors(
    repo_root: &Path,
    test_args: &[String],
    env_keys: &[&str],
) -> Option<Vec<String>> {
    let identity =
        current_python_population_manifest_identity_with_env_keys(repo_root, test_args, env_keys)
            .ok()?;
    let manifest = read_python_population_manifest(repo_root)?;
    let input_fingerprint = python_source_input_fingerprint(repo_root).ok()?;
    if manifest.matches_python_identity(&identity, &normalized_python_repo_root(repo_root))
        && manifest.input_fingerprint == input_fingerprint
    {
        Some(manifest.selectors.clone())
    } else {
        None
    }
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

fn current_python_population_manifest_identity(
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
    let python = PathBuf::from("python");
    Ok(PythonPopulationManifestIdentity {
        cache_schema_version: rslip::CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: PYTHON_SELECTOR_DISCOVERY_VERSION.to_string(),
        python_version: super::super::runners::command_stdout(
            &python,
            &[
                "-c",
                "import sys; print('.'.join(map(str, sys.version_info[:3])))",
            ],
            repo_root,
        )?,
        pytest_version: super::super::runners::command_stdout(
            &python,
            &["-c", "import pytest; print(pytest.__version__)"],
            repo_root,
        )?,
        pytest_args: test_args.to_vec(),
        env: relevant_python_coverage_env(env_keys),
    })
}

fn relevant_python_coverage_env(env_keys: &[&str]) -> BTreeMap<String, String> {
    env_keys
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect()
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
        let mut expected = selectors.to_vec();
        expected.sort();
        expected.dedup();
        self.selectors == expected
    }
}

#[cfg(test)]
#[path = "manifest_test.rs"]
mod manifest_test;
