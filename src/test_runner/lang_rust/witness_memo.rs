#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::test_runner::lang_iface::ExecutionWitness;
use crate::test_runner::rust_coverage_index::rust_coverage_cache_root;

struct WitnessMemo {
    repo: PathBuf,
    stamp: String,
    generation_marker: Option<(String, String, String)>,
    witness: Arc<ExecutionWitness>,
}

fn witness_memo() -> &'static Mutex<Option<WitnessMemo>> {
    static MEMO: OnceLock<Mutex<Option<WitnessMemo>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(None))
}

pub(super) fn file_stamp(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() == 0 {
        return None;
    }
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(format!(
            "meta:{}:{}:{}:{}:{}",
            metadata.len(),
            modified.as_nanos(),
            metadata.dev(),
            metadata.ino(),
            metadata.ctime_nsec()
        ))
    }
    #[cfg(not(unix))]
    Some(format!("meta:{}:{}", metadata.len(), modified.as_nanos()))
}

pub(super) fn stash_published_witness(
    repo_root: &Path,
    witness_path: &Path,
    witness: ExecutionWitness,
) {
    let stamp =
        file_stamp(witness_path).unwrap_or_else(|| format!("gen:{}", witness.generation_id));
    let generation_marker = crate::test_runner::execution_generation::read_pointer(
        &rust_coverage_cache_root(repo_root),
    )
    .ok()
    .flatten()
    .filter(|pointer| pointer.generation_id == witness.generation_id)
    .and_then(|pointer| {
        let manifest = rust_coverage_cache_root(repo_root)
            .join("generations")
            .join(&pointer.generation_id)
            .join("generation.json");
        Some((
            pointer.generation_id,
            pointer.generation_manifest_digest,
            file_stamp(&manifest)?,
        ))
    });
    let key = kiss::rust_include::canonical_path(repo_root);
    if let Ok(mut guard) = witness_memo().lock() {
        *guard = Some(WitnessMemo {
            repo: key,
            stamp,
            generation_marker,
            witness: Arc::new(witness),
        });
    }
}

pub(super) fn memo_witness(repo_root: &Path, witness_path: &Path) -> Option<ExecutionWitness> {
    let key = kiss::rust_include::canonical_path(repo_root);
    let guard = witness_memo().lock().ok()?;
    let memo = guard.as_ref()?;
    if memo.repo != key {
        return None;
    }
    if let Ok(Some(pointer)) =
        crate::test_runner::execution_generation::read_pointer(&rust_coverage_cache_root(repo_root))
    {
        let manifest = rust_coverage_cache_root(repo_root)
            .join("generations")
            .join(&pointer.generation_id)
            .join("generation.json");
        let marker = (
            pointer.generation_id,
            pointer.generation_manifest_digest,
            file_stamp(&manifest)?,
        );
        return (memo.generation_marker.as_ref() == Some(&marker)).then(|| (*memo.witness).clone());
    }
    if memo.generation_marker.is_some() {
        return None;
    }
    match file_stamp(witness_path) {
        Some(stamp) if memo.stamp == stamp => Some((*memo.witness).clone()),
        None if memo.stamp.starts_with("gen:") => Some((*memo.witness).clone()),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn try_recall_published_rust_covered_lines(
    repo_root: &Path,
) -> Option<(String, BTreeMap<String, BTreeSet<u32>>)> {
    let witness_path = rust_coverage_cache_root(repo_root).join("execution_witness.json");
    let key = kiss::rust_include::canonical_path(repo_root);
    let guard = witness_memo().lock().ok()?;
    let memo = guard.as_ref()?;
    if memo.repo != key || !memo.witness.complete {
        return None;
    }
    match file_stamp(&witness_path) {
        Some(stamp) if memo.stamp != stamp => return None,
        None if !memo.stamp.starts_with("gen:") => return None,
        _ => {}
    }
    let covered = memo
        .witness
        .covered_lines
        .iter()
        .map(|(path, lines)| (path.clone(), lines.iter().copied().collect()))
        .collect();
    Some((memo.witness.generation_id.clone(), covered))
}

#[cfg(test)]
pub(super) fn clear_published_witness_memo_for_tests() {
    if let Ok(mut guard) = witness_memo().lock() {
        *guard = None;
    }
}

#[cfg(test)]
#[path = "witness_memo_test.rs"]
mod witness_memo_test;
