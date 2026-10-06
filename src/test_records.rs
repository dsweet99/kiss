use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::rpytest_runner::TestStatus;

pub const RECORD_SCHEMA: &str = "kiss-test-record-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestRecord {
    pub schema: String,
    pub language: String,
    pub test_id: String,
    pub identity: String,
    pub deps: BTreeMap<String, String>,
    pub status: TestStatus,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub covered: BTreeMap<String, BTreeSet<u32>>,
}

impl TestRecord {
    pub fn view(&self) -> RecordView<'_> {
        RecordView {
            identity: &self.identity,
            deps: &self.deps,
            status: self.status,
            duration: self.duration,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RecordView<'a> {
    pub identity: &'a str,
    pub deps: &'a BTreeMap<String, String>,
    pub status: TestStatus,
    pub duration: Duration,
}

#[derive(Clone, Copy, Debug)]
pub struct Selection<'a> {
    pub identity: &'a str,
    pub current_deps: Option<&'a BTreeMap<String, String>>,
    pub retry_bad: bool,
    pub needs_duration: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunReason {
    NoRecord,
    IdentityChanged,
    DepsChanged,
    RetryBad,
    NeedsDuration,
}

pub fn must_run(record: Option<RecordView<'_>>, now: &Selection<'_>) -> Option<RunReason> {
    let Some(record) = record else {
        return Some(RunReason::NoRecord);
    };
    if record.identity != now.identity {
        return Some(RunReason::IdentityChanged);
    }
    if now.current_deps != Some(record.deps) {
        return Some(RunReason::DepsChanged);
    }
    if now.retry_bad && record.status != TestStatus::Passed {
        return Some(RunReason::RetryBad);
    }
    if now.needs_duration && record.duration.is_zero() {
        return Some(RunReason::NeedsDuration);
    }
    None
}

pub fn records_dir(repo_root: &Path, language: &str) -> PathBuf {
    crate::test_state_dir(repo_root)
        .join("records")
        .join(language)
}

pub fn record_path(dir: &Path, test_id: &str) -> PathBuf {
    dir.join(format!("{:016x}.json", fnv1a64(test_id.as_bytes())))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    bytes.iter().fold(OFFSET, |acc, byte| {
        (acc ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

pub fn store_record(dir: &Path, record: &TestRecord) -> io::Result<()> {
    let path = record_path(dir, &record.test_id);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("record.json");
    let tmp = dir.join(format!(
        ".{name}.{}.tmp",
        crate::kiss_publication_barrier::unique_process_suffix()
    ));
    crate::kiss_publication_barrier::publish_atomically_without_parent_sync(
        "test_record",
        &path,
        &tmp,
        |file| {
            serde_json::to_writer(&mut *file, record).map_err(io::Error::other)?;
            file.write_all(b"\n")
        },
    )
}

pub fn load_record(dir: &Path, test_id: &str) -> Option<TestRecord> {
    read_record(&record_path(dir, test_id)).filter(|record| record.test_id == test_id)
}

pub fn load_records(dir: &Path) -> Vec<TestRecord> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut records: Vec<TestRecord> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| read_record(&path))
        .collect();
    records.sort_by(|a, b| a.test_id.cmp(&b.test_id));
    records
}

#[derive(Deserialize)]
struct RecordStatus {
    schema: String,
    test_id: String,
    status: TestStatus,
}

pub fn nonpassed_test_ids(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| serde_json::from_slice::<RecordStatus>(&fs::read(path).ok()?).ok())
        .filter(|record| record.schema == RECORD_SCHEMA && record.status != TestStatus::Passed)
        .map(|record| record.test_id)
        .collect();
    ids.sort();
    ids
}

pub fn read_record(path: &Path) -> Option<TestRecord> {
    let record: TestRecord = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (record.schema == RECORD_SCHEMA).then_some(record)
}

#[cfg(test)]
#[path = "test_records_test.rs"]
mod tests;
