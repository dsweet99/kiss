use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};

use crate::rust_llvm_cov_runner::batch_fingerprint::toolchain_identity;
use crate::rust_llvm_cov_runner::record_digest::{ParsedItems, test_definition_digests};
use crate::rust_llvm_cov_runner::{RustCoverageBatchRequest, RustCoverageToolIdentity};
use crate::test_records::{
    Selection, TestRecord, load_record, load_records, must_run, record_path, records_dir,
};

pub fn rust_record_identity(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> io::Result<String> {
    let root = req
        .source_root
        .canonicalize()
        .unwrap_or_else(|_| req.source_root.clone());
    Ok(format!(
        "{}:{}",
        toolchain_identity(req, tools),
        crate::rust_llvm_cov_runner::plan::shared_input::rust_record_input_digest(&root)?
    ))
}

pub fn rust_selectors_without_records(source_root: &Path, selectors: &[String]) -> Vec<String> {
    let dir = records_dir(source_root, "rust");
    selectors
        .iter()
        .filter(|selector| !record_path(&dir, selector).is_file())
        .cloned()
        .collect()
}

pub fn rust_record_misses(source_root: &Path, identity: &str, selectors: &[String]) -> Vec<String> {
    let dir = records_dir(source_root, "rust");
    let mut files = CurrentFiles::new(canonical_root(source_root));
    selectors
        .iter()
        .filter(|selector| {
            let record = load_record(&dir, selector);
            !holds(&mut files, identity, record.as_ref())
        })
        .cloned()
        .collect()
}

pub fn rust_records_holding(source_root: &Path, identity: &str) -> Vec<TestRecord> {
    let mut files = CurrentFiles::new(canonical_root(source_root));
    load_records(&records_dir(source_root, "rust"))
        .into_iter()
        .filter(|record| holds(&mut files, identity, Some(record)))
        .collect()
}

fn canonical_root(source_root: &Path) -> PathBuf {
    source_root
        .canonicalize()
        .unwrap_or_else(|_| source_root.to_path_buf())
}

pub struct RustRecordDeps(CurrentFiles);

impl RustRecordDeps {
    pub fn new(source_root: &Path) -> Self {
        Self(CurrentFiles::new(canonical_root(source_root)))
    }

    pub fn current(&mut self, record: &TestRecord) -> Option<BTreeMap<String, String>> {
        current_deps(&mut self.0, record)
    }
}

fn holds(files: &mut CurrentFiles, identity: &str, record: Option<&TestRecord>) -> bool {
    let current = record.and_then(|record| current_deps(files, record));
    must_run(
        record.map(TestRecord::view),
        &Selection {
            identity,
            current_deps: current.as_ref(),
            retry_bad: false,
            needs_duration: false,
        },
    )
    .is_none()
}

fn current_deps(files: &mut CurrentFiles, record: &TestRecord) -> Option<BTreeMap<String, String>> {
    record
        .deps
        .keys()
        .map(|key| {
            let digest = match key.rsplit_once("::") {
                Some((rel, _)) => files.definitions(rel)?.get(key)?.clone(),
                None => {
                    let abs = files.root.join(key).to_string_lossy().into_owned();
                    let covered = record.covered.get(&abs)?;
                    files.items(key)?.covered_digest(covered)
                }
            };
            Some((key.clone(), digest))
        })
        .collect()
}

struct CurrentFiles {
    root: PathBuf,
    items: HashMap<String, Option<ParsedItems>>,
    definitions: HashMap<String, Option<BTreeMap<String, String>>>,
}

impl CurrentFiles {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            items: HashMap::new(),
            definitions: HashMap::new(),
        }
    }

    fn items(&mut self, rel: &str) -> Option<&ParsedItems> {
        let root = &self.root;
        self.items
            .entry(rel.to_string())
            .or_insert_with(|| {
                std::fs::read_to_string(root.join(rel))
                    .ok()
                    .map(ParsedItems::parse)
            })
            .as_ref()
    }

    fn definitions(&mut self, rel: &str) -> Option<&BTreeMap<String, String>> {
        if !self.definitions.contains_key(rel) {
            let parsed = std::fs::read_to_string(self.root.join(rel))
                .ok()
                .map(|text| test_definition_digests(rel, &text));
            self.definitions.insert(rel.to_string(), parsed);
        }
        self.definitions.get(rel)?.as_ref()
    }
}

#[cfg(test)]
#[path = "record_select_test.rs"]
mod record_select_test;
