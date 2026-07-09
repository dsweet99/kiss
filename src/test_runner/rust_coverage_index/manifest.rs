use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{
    CACHE_SCHEMA_VERSION, POPULATION_SCHEMA_VERSION, RUST_SELECTOR_DISCOVERY_VERSION,
    command_stdout, create_new_file, entries_fingerprint, normalized_repo_root,
    rust_coverage_cache_root, rust_population_manifest_path, unique_suffix,
    workspace_input_fingerprint,
};

pub(crate) const RUST_COVERAGE_ENV_KEYS: &[&str] = &[
    "RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_TARGET_DIR",
    "LLVM_PROFILE_FILE",
];

pub(crate) fn write_rust_population_manifest_for_args(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> Result<(), String> {
    let identity = current_rust_population_manifest_identity(repo_root, test_args)?;
    write_rust_population_manifest_with_identity(repo_root, selectors, &identity)
}

pub(crate) fn write_rust_population_manifest_with_identity(
    repo_root: &Path,
    selectors: &[String],
    identity: &RustPopulationManifestIdentity,
) -> Result<(), String> {
    let mut selectors = selectors.to_vec();
    selectors.sort();
    selectors.dedup();
    let path = rust_population_manifest_path(repo_root);
    let parent = path.parent().ok_or_else(|| {
        "error: kiss test: Rust population manifest path has no parent".to_string()
    })?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp_path = parent.join(format!(".population.{}.tmp", unique_suffix()));
    let mut file = create_new_file(&tmp_path).map_err(|e| e.to_string())?;
    let payload = RustPopulationManifest {
        schema_version: POPULATION_SCHEMA_VERSION.to_string(),
        cache_schema_version: identity.cache_schema_version.clone(),
        source_root: normalized_repo_root(repo_root),
        selector_discovery_version: identity.selector_discovery_version.clone(),
        rustc_version: identity.rustc_version.clone(),
        cargo_version: identity.cargo_version.clone(),
        cargo_llvm_cov_version: identity.cargo_llvm_cov_version.clone(),
        cargo_args: identity.cargo_args.clone(),
        test_args: identity.test_args.clone(),
        env: identity.env.clone(),
        input_fingerprint: workspace_input_fingerprint(repo_root).map_err(|e| e.to_string())?,
        entries_fingerprint: entries_fingerprint(&rust_coverage_cache_root(repo_root))
            .map_err(|e| e.to_string())?,
        selectors,
    };
    serde_json::to_writer_pretty(&mut file, &payload).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(tmp_path, path).map_err(|e| e.to_string())
}

#[cfg(test)]
pub(crate) fn rust_population_manifest_is_current_for_args(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> bool {
    rust_population_manifest_is_current_for_args_with_env_keys(
        repo_root,
        selectors,
        test_args,
        RUST_COVERAGE_ENV_KEYS,
    )
}

pub(crate) fn rust_population_manifest_is_current_for_args_with_env_keys(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
    env_keys: &[&str],
) -> bool {
    let Ok(identity) =
        current_rust_population_manifest_identity_with_env_keys(repo_root, test_args, env_keys)
    else {
        return false;
    };
    rust_population_manifest_is_current_with_identity(repo_root, selectors, &identity)
}

pub(crate) fn rust_population_manifest_is_current_with_identity(
    repo_root: &Path,
    selectors: &[String],
    identity: &RustPopulationManifestIdentity,
) -> bool {
    let Some(manifest) = read_population_manifest(repo_root) else {
        return false;
    };
    let Ok(input_fingerprint) = workspace_input_fingerprint(repo_root) else {
        return false;
    };
    let Ok(entries_fp) = entries_fingerprint(&rust_coverage_cache_root(repo_root)) else {
        return false;
    };
    manifest.matches_identity(identity, &normalized_repo_root(repo_root))
        && manifest.input_fingerprint == input_fingerprint
        && manifest.entries_fingerprint == entries_fp
        && manifest.matches_selectors(selectors)
}

fn read_population_manifest(repo_root: &Path) -> Option<RustPopulationManifest> {
    fs::read(rust_population_manifest_path(repo_root))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<RustPopulationManifest>(&bytes).ok())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RustPopulationManifestIdentity {
    pub(crate) cache_schema_version: String,
    pub(crate) selector_discovery_version: String,
    pub(crate) rustc_version: String,
    pub(crate) cargo_version: String,
    pub(crate) cargo_llvm_cov_version: String,
    pub(crate) cargo_args: Vec<String>,
    pub(crate) test_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
}

impl RustPopulationManifestIdentity {
    pub(crate) fn tool_versions(&self) -> [&str; 3] {
        [
            self.rustc_version.as_str(),
            self.cargo_version.as_str(),
            self.cargo_llvm_cov_version.as_str(),
        ]
    }

    pub(crate) fn has_tool_versions(&self) -> bool {
        self.tool_versions()
            .iter()
            .all(|version| !version.trim().is_empty())
    }

    pub(crate) fn args_match(&self, cargo_args: &[String], test_args: &[String]) -> bool {
        self.cargo_args == cargo_args && self.test_args == test_args
    }
}

fn current_rust_population_manifest_identity(
    repo_root: &Path,
    test_args: &[String],
) -> Result<RustPopulationManifestIdentity, String> {
    current_rust_population_manifest_identity_with_env_keys(
        repo_root,
        test_args,
        RUST_COVERAGE_ENV_KEYS,
    )
}

fn current_rust_population_manifest_identity_with_env_keys(
    repo_root: &Path,
    test_args: &[String],
    env_keys: &[&str],
) -> Result<RustPopulationManifestIdentity, String> {
    let cargo = PathBuf::from("cargo");
    let rustc = PathBuf::from("rustc");
    Ok(RustPopulationManifestIdentity {
        cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: RUST_SELECTOR_DISCOVERY_VERSION.to_string(),
        rustc_version: command_stdout(&rustc, &["-Vv"], repo_root)?,
        cargo_version: command_stdout(&cargo, &["--version"], repo_root)?,
        cargo_llvm_cov_version: command_stdout(&cargo, &["llvm-cov", "--version"], repo_root)?,
        cargo_args: Vec::new(),
        test_args: test_args.to_vec(),
        env: relevant_rust_coverage_env(env_keys),
    })
}

fn relevant_rust_coverage_env(env_keys: &[&str]) -> BTreeMap<String, String> {
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
pub(crate) struct RustPopulationManifest {
    pub(crate) schema_version: String,
    pub(crate) cache_schema_version: String,
    pub(crate) source_root: String,
    pub(crate) selector_discovery_version: String,
    pub(crate) rustc_version: String,
    pub(crate) cargo_version: String,
    pub(crate) cargo_llvm_cov_version: String,
    pub(crate) cargo_args: Vec<String>,
    pub(crate) test_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) input_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) selectors: Vec<String>,
}

impl RustPopulationManifest {
    pub(crate) fn matches_identity(
        &self,
        identity: &RustPopulationManifestIdentity,
        source_root: &str,
    ) -> bool {
        identity.has_tool_versions()
            && self.schema_version == POPULATION_SCHEMA_VERSION
            && self.cache_schema_version == identity.cache_schema_version
            && self.source_root == source_root
            && self.selector_discovery_version == identity.selector_discovery_version
            && self.rustc_version == identity.rustc_version
            && self.cargo_version == identity.cargo_version
            && self.cargo_llvm_cov_version == identity.cargo_llvm_cov_version
            && identity.args_match(&self.cargo_args, &self.test_args)
            && self.env == identity.env
    }

    pub(crate) fn matches_selectors(&self, selectors: &[String]) -> bool {
        let mut expected = selectors.to_vec();
        expected.sort();
        expected.dedup();
        self.selectors == expected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> RustPopulationManifestIdentity {
        RustPopulationManifestIdentity {
            cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
            selector_discovery_version: RUST_SELECTOR_DISCOVERY_VERSION.to_string(),
            rustc_version: "rustc".to_string(),
            cargo_version: "cargo".to_string(),
            cargo_llvm_cov_version: "llvm-cov".to_string(),
            cargo_args: Vec::new(),
            test_args: Vec::new(),
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn rust_population_manifest_identity() {
        let identity = identity();

        assert!(identity.has_tool_versions());
        assert_eq!(identity.tool_versions(), ["rustc", "cargo", "llvm-cov"]);
        assert!(identity.args_match(&[], &[]));
    }

    #[test]
    fn rust_population_manifest() {
        let identity = identity();
        let manifest = RustPopulationManifest {
            schema_version: POPULATION_SCHEMA_VERSION.to_string(),
            cache_schema_version: identity.cache_schema_version.clone(),
            source_root: "root".to_string(),
            selector_discovery_version: identity.selector_discovery_version.clone(),
            rustc_version: identity.rustc_version.clone(),
            cargo_version: identity.cargo_version.clone(),
            cargo_llvm_cov_version: identity.cargo_llvm_cov_version.clone(),
            cargo_args: Vec::new(),
            test_args: Vec::new(),
            env: BTreeMap::new(),
            input_fingerprint: "input".to_string(),
            entries_fingerprint: "entries".to_string(),
            selectors: vec!["a".to_string(), "b".to_string()],
        };

        assert!(manifest.matches_identity(&identity, "root"));
        assert!(manifest.matches_selectors(&["a".to_string(), "b".to_string()]));
    }
}

#[cfg(test)]
#[path = "manifest_test.rs"]
mod manifest_test;
