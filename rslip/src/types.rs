use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub type CoveringTest = (PathBuf, String);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FileRole {
    Source,
    Test,
    Config,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CoverageMetadata {
    pub executable_lines: Vec<usize>,
    pub executed_lines: Vec<usize>,
    pub missing_lines: Vec<usize>,
    pub percent_covered: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileRecord {
    pub path: String,
    pub role: FileRole,
    pub content_digest: String,
    pub len: u64,
    pub mtime_ns: u128,
    pub coverage: Option<CoverageMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TestRecord {
    pub selector: String,
    pub test_path: String,
    pub content_digest: String,
    pub covered_files: Vec<String>,
    pub covered_lines: BTreeMap<String, Vec<usize>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Database {
    pub schema_version: u32,
    pub rslip_version: String,
    pub config_fingerprints: BTreeMap<String, String>,
    pub files: BTreeMap<String, FileRecord>,
    pub tests: BTreeMap<String, TestRecord>,
    pub source_to_covering_tests: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestCoverageRun {
    pub selector: String,
    pub test_path: PathBuf,
    pub hits: BTreeMap<PathBuf, BTreeSet<usize>>,
}

pub struct PytestTraceCollector;
