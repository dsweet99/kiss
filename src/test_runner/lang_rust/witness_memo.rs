use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::test_runner::lang_iface::ExecutionWitness;
use crate::test_runner::rust_coverage_index::rust_coverage_cache_root;

struct WitnessMemo {
    repo: PathBuf,
    stamp: String,
    witness: Arc<ExecutionWitness>,
}

fn witness_memo() -> &'static Mutex<Option<WitnessMemo>> {
    static MEMO: OnceLock<Mutex<Option<WitnessMemo>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(None))
}

pub(super) fn file_stamp(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let digest = crate::analyze_cache::fnv1a64(0xcbf2_9ce4_8422_2325, &bytes);
    Some(format!("{digest:016x}"))
}

pub(super) fn stash_published_witness(
    repo_root: &Path,
    witness_path: &Path,
    witness: ExecutionWitness,
) {
    let stamp =
        file_stamp(witness_path).unwrap_or_else(|| format!("gen:{}", witness.generation_id));
    let key = kiss::rust_include::canonical_path(repo_root);
    if let Ok(mut guard) = witness_memo().lock() {
        *guard = Some(WitnessMemo {
            repo: key,
            stamp,
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
    match file_stamp(witness_path) {
        Some(stamp) if memo.stamp == stamp => Some((*memo.witness).clone()),
        None if memo.stamp.starts_with("gen:") => Some((*memo.witness).clone()),
        _ => None,
    }
}

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
