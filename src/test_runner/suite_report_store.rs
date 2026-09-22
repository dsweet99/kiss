use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::test_runner::rust_coverage_index::{create_new_file, unique_suffix};

pub(super) const SCHEMA_VERSION: &str = "kiss-suite-report-v6";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct DurableSuiteRecap {
    pub schema_version: String,
    pub ignore: Vec<String>,
    pub extra: Vec<String>,
    pub python_extra: Vec<String>,
    pub digest_all: String,
    pub digest_python: String,
    pub digest_rust: String,
    pub suite: StoredSuite,
    pub recaps: StoredRecaps,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct StoredSuite {
    pub named: Vec<StoredNamed>,
    pub lang_passed: [usize; 2],
    pub lang_failed: [usize; 2],
    pub lang_timed_out: [usize; 2],
    pub anonymous_passed: usize,
    pub anonymous_failed: usize,
    pub anonymous_timed_out: usize,
    pub total_label: String,
    pub max_pass_label: String,
    pub violations: Vec<String>,
    pub gates_clean: bool,
    pub exit_code: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct StoredNamed {
    pub lang: String,
    pub selector: String,
    pub outcome: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct StoredRecaps {
    pub all: Option<StoredRecap>,
    pub python: Option<StoredLangRecap>,
    pub rust: Option<StoredLangRecap>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct StoredRecap {
    pub output: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct StoredLangRecap {
    pub exit_code: i32,
    pub output: String,
}

pub(super) struct FilterIdentity<'a> {
    pub ignore: &'a [String],
    pub extra: &'a [String],
    pub python_extra: &'a [String],
}

pub(super) fn identity_matches(stored: &DurableSuiteRecap, key: &FilterIdentity<'_>) -> bool {
    stored.schema_version == SCHEMA_VERSION
        && stored.ignore == key.ignore
        && stored.extra == key.extra
        && stored.python_extra == key.python_extra
}

pub(super) fn suite_report_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("suite_report.json")
}

pub(super) fn read_store(repo_root: &Path) -> Option<DurableSuiteRecap> {
    let bytes = fs::read(suite_report_path(repo_root)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(super) fn write_store(repo_root: &Path, stored: &DurableSuiteRecap) -> Result<(), String> {
    let path = suite_report_path(repo_root);
    let parent = path
        .parent()
        .ok_or_else(|| "error: kiss test: suite report path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp_path = parent.join(format!(".suite_report.{}.tmp", unique_suffix()));
    let mut file = create_new_file(&tmp_path).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, stored).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(tmp_path, path).map_err(|e| e.to_string())
}
