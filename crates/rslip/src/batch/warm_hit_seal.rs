//! Compact warm-hit seal: skip per-entry opens when a prior all-resolved
//! batch under the same context / selector set still has matching digests.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rpytest_runner::TestStatus;
use serde::{Deserialize, Serialize};

use crate::cache::{digest_recorded_path, rslip_fnv1a64};
use crate::{CacheStatus, LineCoverage, RslipOutcome, RslipRequest};

const SEAL_SCHEMA_VERSION: &str = "rslip-warm-hit-v3";
const SEAL_FILE_NAME: &str = "warm_hit_seal.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    len: u64,
    mtime_nanos: u64,
    digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct WarmHitSeal {
    schema_version: String,
    context_fingerprint: String,
    selectors_fingerprint: String,
    selector_count: usize,
    /// Workspace py/rs files fingerprint from planning; when present and matched
    /// by the request, skips per-file covered stamp checks.
    #[serde(default)]
    content_fingerprint: Option<String>,
    covered_files: BTreeMap<String, FileStamp>,
    failed_nodeids: Vec<String>,
}

fn seal_path(cache_root: &Path) -> PathBuf {
    cache_root.join(SEAL_FILE_NAME)
}

pub fn warm_hit_seal_exists(cache_root: &Path) -> bool {
    seal_path(cache_root).is_file()
}

pub(crate) fn selectors_fingerprint(nodeids: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let mut h = rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"rslip-warm-hit-selectors-v1");
    for nodeid in nodeids {
        h = rslip_fnv1a64(h, nodeid.as_ref().as_bytes());
        h = rslip_fnv1a64(h, &[0]);
    }
    format!("{h:016x}")
}

fn mtime_nanos(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn stamp_for_path(source_root: &Path, recorded: &str, digest: &str) -> Option<FileStamp> {
    let path = if Path::new(recorded).is_absolute() {
        PathBuf::from(recorded)
    } else {
        source_root.join(recorded)
    };
    let meta = fs::metadata(path).ok()?;
    Some(FileStamp {
        len: meta.len(),
        mtime_nanos: mtime_nanos(&meta),
        digest: digest.to_string(),
    })
}

fn covered_files_still_match(source_root: &Path, files: &BTreeMap<String, FileStamp>) -> bool {
    files.iter().all(|(recorded, stamp)| {
        let path = if Path::new(recorded).is_absolute() {
            PathBuf::from(recorded)
        } else {
            source_root.join(recorded)
        };
        let Ok(meta) = fs::metadata(&path) else {
            return false;
        };
        if meta.len() == stamp.len && mtime_nanos(&meta) == stamp.mtime_nanos {
            return true;
        }
        digest_recorded_path(source_root, recorded).as_ref() == Some(&stamp.digest)
    })
}

fn content_fingerprint_matches(reqs: &[RslipRequest], seal: &WarmHitSeal) -> bool {
    let Some(seal_fp) = seal.content_fingerprint.as_deref() else {
        return false;
    };
    let Some(first_fp) = reqs.first().and_then(|req| req.content_fingerprint.as_deref()) else {
        return false;
    };
    if first_fp != seal_fp {
        return false;
    }
    reqs.iter()
        .all(|req| req.content_fingerprint.as_deref() == Some(seal_fp))
}

fn upgrade_warm_hit_seal_content_fingerprint(
    cache_root: &Path,
    seal: &WarmHitSeal,
    content_fingerprint: String,
) -> io::Result<()> {
    if seal.schema_version == SEAL_SCHEMA_VERSION
        && seal.content_fingerprint.as_ref() == Some(&content_fingerprint)
    {
        return Ok(());
    }
    let mut upgraded = seal.clone();
    upgraded.schema_version = SEAL_SCHEMA_VERSION.to_string();
    upgraded.content_fingerprint = Some(content_fingerprint);
    let path = seal_path(cache_root);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp.{nanos}"));
    let mut file = fs::File::create(&tmp)?;
    serde_json::to_writer(&mut file, &upgraded).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}

/// When the seal is valid, synthesize hit outcomes without opening entries.
pub(crate) fn try_warm_hit_seal(
    reqs: &[RslipRequest],
    context_fingerprint: &str,
) -> Option<Vec<RslipOutcome>> {
    let first = reqs.first()?;
    if reqs.iter().any(|req| req.force_rerun) {
        return None;
    }
    if !reqs.iter().all(|req| {
        req.cache_root == first.cache_root && req.source_root == first.source_root
    }) {
        return None;
    }
    let bytes = fs::read(seal_path(&first.cache_root)).ok()?;
    let seal: WarmHitSeal = serde_json::from_slice(&bytes).ok()?;
    let selectors_fp = selectors_fingerprint(reqs.iter().map(|req| req.nodeid.as_str()));
    let files_ok = content_fingerprint_matches(reqs, &seal)
        || covered_files_still_match(&first.source_root, &seal.covered_files);
    let schema_ok = seal.schema_version == SEAL_SCHEMA_VERSION
        || seal.schema_version == "rslip-warm-hit-v2";
    let ok = schema_ok
        && seal.context_fingerprint == context_fingerprint
        && seal.selector_count == reqs.len()
        && seal.selectors_fingerprint == selectors_fp
        && files_ok;
    if !ok {
        return None;
    }
    if let Some(fp) = first.content_fingerprint.clone() {
        let _ = upgrade_warm_hit_seal_content_fingerprint(&first.cache_root, &seal, fp);
    }
    let failed: BTreeSet<&str> = seal.failed_nodeids.iter().map(String::as_str).collect();
    Some(
        reqs.iter()
            .map(|req| {
                let failed = failed.contains(req.nodeid.as_str());
                RslipOutcome {
                    nodeid: req.nodeid.clone(),
                    status: if failed {
                        TestStatus::Failed
                    } else {
                        TestStatus::Passed
                    },
                    exit_code: Some(if failed { 1 } else { 0 }),
                    duration: Duration::ZERO,
                    coverage: LineCoverage {
                        files: BTreeMap::new(),
                    },
                    cache_status: CacheStatus::Hit,
                    stdout: None,
                    stderr: None,
                }
            })
            .collect(),
    )
}

pub(crate) fn write_warm_hit_seal(
    cache_root: &Path,
    source_root: &Path,
    context_fingerprint: &str,
    nodeids: &[String],
    outcomes: &[RslipOutcome],
    covered_digests: BTreeMap<String, String>,
    content_fingerprint: Option<String>,
) -> io::Result<()> {
    if nodeids.len() != outcomes.len() {
        return Err(io::Error::other(
            "warm hit seal requires aligned nodeids and outcomes",
        ));
    }
    let mut covered_files = BTreeMap::new();
    for (recorded, digest) in &covered_digests {
        let Some(stamp) = stamp_for_path(source_root, recorded, digest) else {
            continue;
        };
        covered_files.insert(recorded.clone(), stamp);
    }
    if covered_files.is_empty() {
        return Err(io::Error::other("warm hit seal requires covered file stamps"));
    }
    let failed_nodeids = outcomes
        .iter()
        .filter(|outcome| outcome.status != TestStatus::Passed)
        .map(|outcome| outcome.nodeid.clone())
        .collect::<Vec<_>>();
    let seal = WarmHitSeal {
        schema_version: SEAL_SCHEMA_VERSION.to_string(),
        context_fingerprint: context_fingerprint.to_string(),
        selectors_fingerprint: selectors_fingerprint(nodeids.iter().map(String::as_str)),
        selector_count: nodeids.len(),
        content_fingerprint,
        covered_files,
        failed_nodeids,
    };
    let path = seal_path(cache_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp.{nanos}"));
    let mut file = fs::File::create(&tmp)?;
    serde_json::to_writer(&mut file, &seal).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rslip_sample_request;
    use std::fs;

    #[test]
    fn warm_hit_seal_round_trips_and_detects_digest_drift() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app.py");
        fs::write(&app, "x = 1\n").unwrap();
        let mut req = rslip_sample_request(tmp.path());
        req.nodeid = "test_sample.py::test_ok".to_string();
        req.cache_root = tmp.path().join("cache");
        fs::create_dir_all(&req.cache_root).unwrap();
        let context = "ctx-1";
        let digests = BTreeMap::from([(
            "app.py".to_string(),
            digest_recorded_path(tmp.path(), "app.py").unwrap(),
        )]);
        let outcomes = vec![RslipOutcome {
            nodeid: req.nodeid.clone(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::ZERO,
            coverage: LineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: CacheStatus::Hit,
            stdout: None,
            stderr: None,
        }];
        write_warm_hit_seal(
            &req.cache_root,
            tmp.path(),
            context,
            std::slice::from_ref(&req.nodeid),
            &outcomes,
            digests,
            None,
        )
        .unwrap();
        assert!(try_warm_hit_seal(std::slice::from_ref(&req), context).is_some());
        // Change length so the mtime/len fast path cannot mask digest drift
        // on coarse-resolution filesystems (same-length rewrite can keep len).
        fs::write(&app, "x = 22\n").unwrap();
        assert!(try_warm_hit_seal(std::slice::from_ref(&req), context).is_none());
    }

    #[test]
    fn warm_hit_seal_accepts_matching_content_fingerprint_without_restat() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app.py");
        fs::write(&app, "x = 1\n").unwrap();
        let mut req = rslip_sample_request(tmp.path());
        req.nodeid = "test_sample.py::test_ok".to_string();
        req.cache_root = tmp.path().join("cache");
        req.content_fingerprint = Some("fp-abc".to_string());
        fs::create_dir_all(&req.cache_root).unwrap();
        let context = "ctx-1";
        let digests = BTreeMap::from([(
            "app.py".to_string(),
            digest_recorded_path(tmp.path(), "app.py").unwrap(),
        )]);
        let outcomes = vec![RslipOutcome {
            nodeid: req.nodeid.clone(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::ZERO,
            coverage: LineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: CacheStatus::Hit,
            stdout: None,
            stderr: None,
        }];
        write_warm_hit_seal(
            &req.cache_root,
            tmp.path(),
            context,
            std::slice::from_ref(&req.nodeid),
            &outcomes,
            digests,
            Some("fp-abc".to_string()),
        )
        .unwrap();
        // Delete the covered file so per-file stamp checks would fail; content
        // fingerprint match must still allow the warm hit.
        fs::remove_file(&app).unwrap();
        assert!(try_warm_hit_seal(std::slice::from_ref(&req), context).is_some());
        req.content_fingerprint = Some("fp-other".to_string());
        assert!(try_warm_hit_seal(std::slice::from_ref(&req), context).is_none());
    }
}
