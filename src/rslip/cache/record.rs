use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use super::{RslipCacheEntry, entry_is_reusable, rslip_request_context_fingerprint};
use crate::rslip::{LineCoverage, RslipRequest};
use crate::test_records::{
    RECORD_SCHEMA, RecordView, Selection, TestRecord, load_record, must_run, records_dir,
    store_record,
};

pub fn python_records_dir(source_root: &Path) -> PathBuf {
    records_dir(source_root, "python")
}

pub(crate) fn load_rslip_record(req: &RslipRequest) -> Option<TestRecord> {
    let identity = rslip_request_context_fingerprint(req).ok()?;
    load_record(&python_records_dir(&req.source_root), &req.nodeid)
        .filter(|record| record.identity == identity)
}

pub(crate) fn load_rslip_cache_entry(req: &RslipRequest) -> Option<RslipCacheEntry> {
    load_rslip_record(req).map(RslipCacheEntry::from_record)
}

pub(crate) fn load_reusable_rslip_cache_entry(req: &RslipRequest) -> Option<RslipCacheEntry> {
    load_rslip_cache_entry(req).filter(|entry| entry_is_reusable(entry, &req.source_root))
}

pub(crate) fn store_rslip_cache_entry(
    req: &RslipRequest,
    entry: &RslipCacheEntry,
) -> io::Result<()> {
    let identity = rslip_request_context_fingerprint(req)?;
    store_record(
        &python_records_dir(&req.source_root),
        &entry.to_record(&identity),
    )
}

impl RslipCacheEntry {
    pub(crate) fn from_record(record: TestRecord) -> Self {
        Self {
            nodeid: record.test_id,
            status: record.status,
            exit_code: record.exit_code,
            duration: record.duration,
            coverage: LineCoverage {
                files: record.covered,
            },
            covered_digests: record.deps,
        }
    }

    pub(crate) fn to_record(&self, identity: &str) -> TestRecord {
        TestRecord {
            schema: RECORD_SCHEMA.to_string(),
            language: "python".to_string(),
            test_id: self.nodeid.clone(),
            identity: identity.to_string(),
            deps: self.covered_digests.clone(),
            status: self.status,
            exit_code: self.exit_code,
            duration: self.duration,
            covered: self.coverage.files.clone(),
        }
    }
}

pub(super) fn deps_still_hold(
    entry: &RslipCacheEntry,
    current: Option<&BTreeMap<String, String>>,
) -> bool {
    let view = RecordView {
        identity: "",
        deps: &entry.covered_digests,
        status: entry.status,
        duration: entry.duration,
    };
    let now = Selection {
        identity: "",
        current_deps: current,
        retry_bad: false,
        needs_duration: false,
    };
    must_run(Some(view), &now).is_none()
}
