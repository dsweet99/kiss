#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use std::path::{Path, PathBuf};

#[cfg(test)]
use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::storage::write_test_json_atomically;
#[cfg(test)]
use super::{CACHE_SCHEMA_VERSION, RUST_SELECTOR_DISCOVERY_VERSION, command_stdout};
#[cfg(test)]
use super::{
    LEGACY_POPULATION_SCHEMA_VERSION, POPULATION_SCHEMA_VERSION, normalized_repo_root,
    rust_coverage_cache_root, rust_coverage_index_path, rust_population_manifest_path,
    unique_suffix,
};
#[cfg(test)]
use std::fs;

pub(crate) const RUST_COVERAGE_ENV_KEYS: &[&str] = &[
    "RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_TARGET_DIR",
    "LLVM_PROFILE_FILE",
];

#[cfg(test)]
pub(crate) fn write_rust_population_manifest_for_args(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> Result<(), String> {
    let identity = current_rust_population_manifest_identity(repo_root, test_args)?;
    write_rust_population_manifest_with_identity(repo_root, selectors, &identity)
}

#[cfg(test)]
fn write_test_derived_state_files(
    repo_root: &Path,
    batch_identity: &rust_llvm_cov_runner::RustCoverageBatchIdentity,
    entries_fingerprint: &str,
    selectors: &[String],
) -> Result<(), String> {
    let cache_root = rust_coverage_cache_root(repo_root);
    fs::create_dir_all(&cache_root).map_err(|e| e.to_string())?;
    let index_files = super::build_test_rust_coverage_index(repo_root)?;
    write_test_json_atomically(
        &cache_root.join(format!(".index.{}.tmp", unique_suffix())),
        &rust_coverage_index_path(repo_root),
        &serde_json::json!({
            "schema_version": super::INDEX_SCHEMA_VERSION,
            "source_root": normalized_repo_root(repo_root),
            "generation_fingerprint": batch_identity.generation_fingerprint,
            "entries_fingerprint": entries_fingerprint,
            "files": index_files,
        }),
    )?;
    write_test_json_atomically(
        &cache_root.join(format!(".population.{}.tmp", unique_suffix())),
        &rust_population_manifest_path(repo_root),
        &serde_json::json!({
            "schema_version": POPULATION_SCHEMA_VERSION,
            "source_root": normalized_repo_root(repo_root),
            "input_fingerprint": batch_identity.input_digest,
            "generation_fingerprint": batch_identity.generation_fingerprint,
            "selection_context_fingerprint": batch_identity.selection_context_fingerprint,
            "entries_fingerprint": entries_fingerprint,
            "selectors": selectors,
            "ordinary_source_digests": batch_identity
                .ordinary_source_digests
                .iter()
                .map(|(path, digest)| serde_json::json!({ "path": path, "digest": digest }))
                .collect::<Vec<_>>(),
            "test_binaries": [{
                "id": "test-bin",
                "executable": "test-bin",
                "digest": "0000000000000000",
            }],
        }),
    )
}

#[cfg(test)]
pub(crate) fn write_rust_population_manifest_with_identity(
    repo_root: &Path,
    selectors: &[String],
    identity: &RustPopulationManifestIdentity,
) -> Result<(), String> {
    let mut selectors = selectors.to_vec();
    selectors.sort();
    selectors.dedup();
    let batch_identity =
        super::current_rust_coverage_batch_identity(repo_root, &identity.test_args)?;
    let entries_fingerprint = rust_llvm_cov_runner::generation_entries_fingerprint(
        &rust_coverage_cache_root(repo_root),
        &batch_identity.generation_fingerprint,
    )
    .map_err(|e| e.to_string())?;
    write_test_derived_state_files(repo_root, &batch_identity, &entries_fingerprint, &selectors)
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

#[cfg(test)]
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

#[cfg(test)]
pub(crate) fn rust_population_manifest_is_current_with_identity(
    repo_root: &Path,
    selectors: &[String],
    identity: &RustPopulationManifestIdentity,
) -> bool {
    batch_population_manifest_is_current(repo_root, selectors, identity)
}

#[cfg(test)]
fn batch_population_manifest_is_current(
    repo_root: &Path,
    selectors: &[String],
    identity: &RustPopulationManifestIdentity,
) -> bool {
    let Ok(current_identity) =
        super::current_rust_coverage_batch_identity(repo_root, &identity.test_args)
    else {
        return false;
    };
    let mut expected = selectors.to_vec();
    expected.sort();
    expected.dedup();
    let cache_root = super::rust_coverage_cache_root(repo_root);
    rust_llvm_cov_runner::load_current_population_state(
        &cache_root,
        repo_root,
        &current_identity,
        Some(&expected),
    )
    .is_some()
}

#[cfg(test)]
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

#[cfg(test)]
impl RustPopulationManifestIdentity {
    #[cfg(test)]
    pub(crate) fn tool_versions(&self) -> [&str; 3] {
        [
            self.rustc_version.as_str(),
            self.cargo_version.as_str(),
            self.cargo_llvm_cov_version.as_str(),
        ]
    }

    #[cfg(test)]
    pub(crate) fn has_tool_versions(&self) -> bool {
        self.tool_versions()
            .iter()
            .all(|version| !version.trim().is_empty())
    }

    #[cfg(test)]
    pub(crate) fn args_match(&self, cargo_args: &[String], test_args: &[String]) -> bool {
        self.cargo_args == cargo_args && self.test_args == test_args
    }
}

#[cfg(test)]
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

#[cfg(test)]
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
        cargo_args: vec!["--workspace".to_string()],
        test_args: test_args.to_vec(),
        env: relevant_rust_coverage_env(env_keys),
    })
}

#[cfg(test)]
fn relevant_rust_coverage_env(env_keys: &[&str]) -> BTreeMap<String, String> {
    kiss::env_map_from_allowlist(env_keys)
}

#[cfg(test)]
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

#[cfg(test)]
impl RustPopulationManifest {
    pub(crate) fn matches_identity(
        &self,
        identity: &RustPopulationManifestIdentity,
        source_root: &str,
    ) -> bool {
        identity.has_tool_versions()
            && self.schema_version == LEGACY_POPULATION_SCHEMA_VERSION
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
#[path = "manifest_test.rs"]
mod manifest_test;
