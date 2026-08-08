use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use kiss::Language;
use serde::{Deserialize, Serialize};

use super::rust_coverage_index::{create_new_file, unique_suffix};

const LAST_STATUS_SCHEMA_VERSION: &str = "kiss-test-last-status-v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct LastStatusIdentity {
    schema_version: String,
    tool_versions: BTreeMap<String, String>,
    test_args: Vec<String>,
    env: BTreeMap<String, String>,
}

impl LastStatusIdentity {
    fn new(
        tool_versions: BTreeMap<String, String>,
        test_args: &[String],
        env: BTreeMap<String, String>,
    ) -> Self {
        Self {
            schema_version: LAST_STATUS_SCHEMA_VERSION.to_string(),
            tool_versions,
            test_args: test_args.to_vec(),
            env,
        }
    }
}

pub(crate) fn python_last_status_identity(
    python_version: &str,
    pytest_version: &str,
    test_args: &[String],
) -> LastStatusIdentity {
    LastStatusIdentity::new(
        BTreeMap::from([
            ("python".to_string(), python_version.to_string()),
            ("pytest".to_string(), pytest_version.to_string()),
        ]),
        test_args,
        BTreeMap::new(),
    )
}

pub(crate) fn rust_last_status_identity(
    cargo_version: &str,
    llvm_cov_version: &str,
    rustc_version: &str,
    cargo_nextest_version: &str,
    test_args: &[String],
    runner_map_fingerprint: &str,
) -> LastStatusIdentity {
    LastStatusIdentity::new(
        BTreeMap::from([
            ("cargo".to_string(), cargo_version.to_string()),
            ("cargo-llvm-cov".to_string(), llvm_cov_version.to_string()),
            ("rustc".to_string(), rustc_version.to_string()),
            (
                "cargo-nextest".to_string(),
                cargo_nextest_version.to_string(),
            ),
            (
                "cache-schema".to_string(),
                rust_llvm_cov_runner::CACHE_SCHEMA_VERSION.to_string(),
            ),
            (
                "execution-policy".to_string(),
                rust_llvm_cov_runner::BATCH_EXECUTION_POLICY_VERSION.to_string(),
            ),
            ("runner-map".to_string(), runner_map_fingerprint.to_string()),
        ]),
        test_args,
        BTreeMap::new(),
    )
}

pub(crate) fn last_status_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("test_last_status.json")
}

pub(crate) fn has_language_records(repo_root: &Path, language: Language) -> Result<bool, String> {
    let language = match language {
        Language::Python => "python",
        Language::Rust => "rust",
    };
    Ok(read_store(repo_root)?
        .records
        .iter()
        .any(|record| record.language == language))
}

pub(crate) fn prior_failures(
    repo_root: &Path,
    language: Language,
    identity: &LastStatusIdentity,
) -> Result<Vec<String>, String> {
    let language = match language {
        Language::Python => "python",
        Language::Rust => "rust",
    };
    let mut selectors: Vec<_> = read_store(repo_root)?
        .records
        .into_iter()
        .filter(|record| record.language == language && record.identity == *identity)
        .map(|record| record.selector)
        .collect();
    selectors.sort();
    selectors.dedup();
    Ok(selectors)
}

pub(crate) fn record_statuses(
    repo_root: &Path,
    language: Language,
    identity: &LastStatusIdentity,
    statuses: &[(String, rpytest_runner::TestStatus)],
) -> Result<(), String> {
    if statuses.is_empty() {
        return Ok(());
    }
    let language = match language {
        Language::Python => "python",
        Language::Rust => "rust",
    };
    let mut store = read_store(repo_root)?;
    for (selector, status) in statuses {
        store.records.retain(|record| {
            !(record.language == language
                && record.selector == *selector
                && record.identity == *identity)
        });
        if matches!(
            *status,
            rpytest_runner::TestStatus::Failed | rpytest_runner::TestStatus::TimedOut
        ) {
            store.records.push(LastStatusRecord {
                language: language.to_string(),
                selector: selector.clone(),
                identity: identity.clone(),
            });
        }
    }
    store.records.sort_by(|left, right| {
        left.language
            .cmp(&right.language)
            .then_with(|| left.selector.cmp(&right.selector))
    });
    write_store(repo_root, &store)
}

#[derive(Default, Deserialize, Serialize)]
struct LastStatusStore {
    schema_version: String,
    records: Vec<LastStatusRecord>,
}

#[derive(Clone, Deserialize, Serialize)]
struct LastStatusRecord {
    language: String,
    selector: String,
    identity: LastStatusIdentity,
}

fn read_store(repo_root: &Path) -> Result<LastStatusStore, String> {
    let path = last_status_path(repo_root);
    if !path.exists() {
        return Ok(LastStatusStore {
            schema_version: LAST_STATUS_SCHEMA_VERSION.to_string(),
            records: Vec::new(),
        });
    }
    let bytes = fs::read(&path).map_err(|e| {
        format!(
            "error: kiss test: failed to read last-status store {}: {e}",
            path.display()
        )
    })?;
    let store: LastStatusStore = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "error: kiss test: failed to parse last-status store {}: {e}",
            path.display()
        )
    })?;
    if store.schema_version != LAST_STATUS_SCHEMA_VERSION {
        return Err(format!(
            "error: kiss test: unsupported last-status schema {}",
            store.schema_version
        ));
    }
    Ok(store)
}

fn write_store(repo_root: &Path, store: &LastStatusStore) -> Result<(), String> {
    let path = last_status_path(repo_root);
    let parent = path
        .parent()
        .ok_or_else(|| "error: kiss test: last-status path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp_path = parent.join(format!(".test_last_status.{}.tmp", unique_suffix()));
    let mut file = create_new_file(&tmp_path).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, store).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(tmp_path, path).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "last_status_test.rs"]
mod tests;
