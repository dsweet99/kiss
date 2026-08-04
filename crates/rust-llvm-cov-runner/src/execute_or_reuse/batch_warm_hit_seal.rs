//! Compact warm all-hit seal: skip per-entry opens when a prior all-passed
//! batch under the same generation / selector set is still valid.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::plan::batch_fingerprint::RustCoverageBatchIdentity;
use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::rust_cov_fnv1a64;

const SEAL_SCHEMA_VERSION: &str = "rust-warm-all-hit-v1";
const SEAL_FILE_NAME: &str = "warm_all_hit_seal.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct WarmAllHitSeal {
    schema_version: String,
    generation_fingerprint: String,
    entries_fingerprint: String,
    selectors_fingerprint: String,
    selector_count: usize,
    all_passed: bool,
}

fn seal_path(cache_root: &Path) -> PathBuf {
    cache_root.join(SEAL_FILE_NAME)
}

pub(crate) fn selectors_fingerprint(selectors: &[String]) -> String {
    let mut h = rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"warm-all-hit-selectors-v1");
    for selector in selectors {
        h = rust_cov_fnv1a64(h, selector.as_bytes());
        h = rust_cov_fnv1a64(h, &[0]);
    }
    format!("{h:016x}")
}

pub(crate) fn try_warm_all_hit_seal(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
) -> Option<bool> {
    let bytes = fs::read(seal_path(&req.cache_root)).ok()?;
    let seal: WarmAllHitSeal = serde_json::from_slice(&bytes).ok()?;
    let entry_state = crate::publish_derived::batch_entry_state::read_entry_state(&req.cache_root)?;
    let selectors_fp = selectors_fingerprint(&req.logical_selectors);
    let ok = seal.schema_version == SEAL_SCHEMA_VERSION
        && seal.all_passed
        && seal.generation_fingerprint == identity.generation_fingerprint
        && seal.selector_count == req.logical_selectors.len()
        && seal.selectors_fingerprint == selectors_fp
        && entry_state.generation_fingerprint == identity.generation_fingerprint
        && seal.entries_fingerprint == entry_state.entries_fingerprint;
    ok.then_some(true)
}

pub(crate) fn write_warm_all_hit_seal(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
    all_passed: bool,
) -> io::Result<()> {
    let entry_state = crate::publish_derived::batch_entry_state::read_entry_state(&req.cache_root).ok_or_else(|| {
        io::Error::other("warm all-hit seal requires entry_state.json")
    })?;
    if entry_state.generation_fingerprint != identity.generation_fingerprint {
        return Err(io::Error::other(
            "warm all-hit seal generation mismatch with entry_state",
        ));
    }
    let seal = WarmAllHitSeal {
        schema_version: SEAL_SCHEMA_VERSION.to_string(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        entries_fingerprint: entry_state.entries_fingerprint,
        selectors_fingerprint: selectors_fingerprint(&req.logical_selectors),
        selector_count: req.logical_selectors.len(),
        all_passed,
    };
    let path = seal_path(&req.cache_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp.{nanos}"));
    let mut file = File::create(&tmp)?;
    serde_json::to_writer(&mut file, &seal).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}
