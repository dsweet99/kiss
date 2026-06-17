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

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn witness_rslip_types() {
        use crate::{RSLIP_VERSION, SCHEMA_VERSION, content_digest};
        let record = FileRecord {
            path: "a.py".to_string(),
            role: FileRole::Source,
            content_digest: content_digest(b"a"),
            len: 1,
            mtime_ns: 0,
            coverage: Some(CoverageMetadata::default()),
        };
        assert_eq!(record.role, FileRole::Source);

        let test = TestRecord {
            selector: "t.py::test_x".to_string(),
            test_path: "t.py".to_string(),
            content_digest: content_digest(b"t"),
            covered_files: vec!["a.py".to_string()],
            covered_lines: BTreeMap::new(),
        };
        assert_eq!(test.selector, "t.py::test_x");

        let db = Database {
            schema_version: SCHEMA_VERSION,
            rslip_version: RSLIP_VERSION.to_string(),
            config_fingerprints: BTreeMap::new(),
            files: BTreeMap::from([(record.path.clone(), record)]),
            tests: BTreeMap::from([(test.selector.clone(), test)]),
            source_to_covering_tests: BTreeMap::new(),
        };
        let json = serde_json::to_string(&db).unwrap();
        let round_trip: Database = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.schema_version, SCHEMA_VERSION);

        let run = TestCoverageRun {
            selector: "t.py::test_x".to_string(),
            test_path: PathBuf::from("t.py"),
            hits: BTreeMap::new(),
        };
        assert_eq!(run.selector, "t.py::test_x");

        let collector = PytestTraceCollector;
        let _ = std::any::type_name_of_val(&collector);
    }
}
