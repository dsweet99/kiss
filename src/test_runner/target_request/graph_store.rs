use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::digest::digest_bytes;

const SCHEMA: &str = "graph-evidence-v3";
/// Cap retained evidence keys (aligned with target-plans / target-reports).
const ENTRY_LIMIT: usize = 64;

#[cfg(test)]
static EVIDENCE_SOURCE_READS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GraphOrphanItem {
    pub file: String,
    pub unit_name: String,
    #[serde(default)]
    pub start_line: u32,
    #[serde(default)]
    pub end_line: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredGraph {
    schema: String,
    key: String,
    items: Vec<GraphOrphanItem>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoreMeta {
    /// Oldest-first publish order; newest key is last.
    keys: Vec<String>,
}

pub(crate) fn evidence_key(
    repo_root: &Path,
    py: &[PathBuf],
    rs: &[PathBuf],
    orphan_allowed: &[String],
    covered: &BTreeMap<String, BTreeSet<u32>>,
    config: &str,
) -> String {
    let mut files = Vec::new();
    for path in py.iter().chain(rs) {
        let rel = path
            .strip_prefix(repo_root)
            .map(|item| item.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string_lossy().into_owned());
        #[cfg(test)]
        EVIDENCE_SOURCE_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let digest = fs::read(path)
            .map(|bytes| digest_bytes(&bytes))
            .unwrap_or_default();
        files.push((rel, digest));
    }
    files.sort();
    let payload = serde_json::json!({
        "schema": SCHEMA,
        "files": files,
        "orphan_allowed": orphan_allowed,
        "covered": covered,
        "config": config,
    });
    digest_bytes(&serde_json::to_vec(&payload).expect("graph evidence key"))
}

/// Covered + config (+ orphan_allowed) half of `evidence_key`, without source bytes.
/// After a worktree match, sources are already validated; ready freshness uses this.
pub(crate) fn evidence_mutable_digest(
    orphan_allowed: &[String],
    covered: &BTreeMap<String, BTreeSet<u32>>,
    config: &str,
) -> String {
    let payload = serde_json::json!({
        "schema": SCHEMA,
        "orphan_allowed": orphan_allowed,
        "covered": covered,
        "config": config,
    });
    digest_bytes(&serde_json::to_vec(&payload).expect("graph evidence mutable"))
}

/// Presence without full orphan-item deserialize: meta membership, else thin identity.
pub(crate) fn evidence_present(repo_root: &Path, key: &str) -> bool {
    let dir = store_dir(repo_root);
    if !dir.is_dir() {
        return false;
    }
    let Ok(_lock) = lock_store(&dir) else {
        return false;
    };
    let meta = read_meta(&dir);
    if meta.keys.iter().any(|item| item == key) && entry_path(repo_root, key).is_file() {
        return true;
    }
    read_matching_identity(key, &entry_path(repo_root, key)).is_some()
        || read_matching_identity(key, &legacy_entry_path(repo_root, key)).is_some()
}

#[derive(Deserialize)]
struct StoredGraphIdentity {
    schema: String,
    key: String,
}

fn read_matching_identity(key: &str, path: &Path) -> Option<()> {
    let stored: StoredGraphIdentity = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (stored.schema == SCHEMA && stored.key == key).then_some(())
}

#[cfg(test)]
pub(crate) fn reset_evidence_source_reads() {
    EVIDENCE_SOURCE_READS.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn evidence_source_reads() -> usize {
    EVIDENCE_SOURCE_READS.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn load_items(repo_root: &Path, key: &str) -> Option<Vec<GraphOrphanItem>> {
    let dir = store_dir(repo_root);
    if !dir.is_dir() {
        return None;
    }
    // Hold the same exclusive flock as store_items so concurrent kiss test / watch
    // publishers cannot tear dual-path (full-key + legacy) reads mid-migrate.
    let _lock = lock_store(&dir).ok()?;
    // Prefer full-key path; fall back to legacy 16-hex filename with equality so
    // upgraded kiss test processes still reuse pre-fix evidence (fail-closed on mismatch).
    read_matching_items(key, &entry_path(repo_root, key))
        .or_else(|| read_matching_items(key, &legacy_entry_path(repo_root, key)))
}

fn read_matching_items(key: &str, path: &Path) -> Option<Vec<GraphOrphanItem>> {
    let stored: StoredGraph = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (stored.schema == SCHEMA && stored.key == key).then_some(stored.items)
}

pub(crate) fn store_items(
    repo_root: &Path,
    key: &str,
    items: Vec<GraphOrphanItem>,
) -> Result<(), String> {
    let dir = store_dir(repo_root);
    fs::create_dir_all(&dir).map_err(|err| format!("graph evidence store: {err}"))?;
    // Serialize concurrent kiss test / watch publishers (VISION: multi-process, no corruption).
    let _lock = lock_store(&dir)?;
    let stored = StoredGraph {
        schema: SCHEMA.into(),
        key: key.into(),
        items,
    };
    let path = entry_path(repo_root, key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("graph evidence store: {err}"))?;
    }
    // Full-key tmp name: colliding 16-hex prefixes must not share one tmp path under
    // concurrent kiss test / watch assembly writers.
    let tmp = publish_tmp_path(&dir, key);
    let bytes = serde_json::to_vec(&stored).map_err(|err| format!("graph evidence json: {err}"))?;
    {
        let mut file = File::create(&tmp).map_err(|err| format!("graph evidence write: {err}"))?;
        file.write_all(&bytes)
            .map_err(|err| format!("graph evidence write: {err}"))?;
    }
    fs::rename(tmp, path).map_err(|err| format!("graph evidence publish: {err}"))?;
    // Drop legacy truncated file only when it held this same identity (peer keys that
    // still share the prefix path are left for their own equality-checked loads).
    let legacy = legacy_entry_path(repo_root, key);
    if read_matching_items(key, &legacy).is_some() {
        let _ = fs::remove_file(&legacy);
    }
    // Track published keys and prune oldest above ENTRY_LIMIT (kt_bug.md retention).
    let mut meta = read_meta(&dir);
    meta.keys.retain(|item| item != key);
    meta.keys.push(key.to_string());
    prune_unlocked(repo_root, &mut meta);
    write_meta(&dir, &meta)?;
    Ok(())
}

fn store_dir(repo_root: &Path) -> PathBuf {
    repo_root
        .join("target")
        .join("kiss-plan")
        .join("graph-evidence")
}

fn entry_path(repo_root: &Path, key: &str) -> PathBuf {
    store_dir(repo_root).join(format!("{key}.json"))
}

fn legacy_entry_path(repo_root: &Path, key: &str) -> PathBuf {
    store_dir(repo_root).join(format!("{}.json", &key[..16.min(key.len())]))
}

fn publish_tmp_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!(".{key}.tmp"))
}

fn lock_store(dir: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("lock"))
        .map_err(|err| format!("graph evidence lock: {err}"))?;
    file.lock_exclusive()
        .map_err(|err| format!("graph evidence lock: {err}"))?;
    Ok(file)
}

fn read_meta(dir: &Path) -> StoreMeta {
    fs::read(dir.join("meta.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_meta(dir: &Path, meta: &StoreMeta) -> Result<(), String> {
    let path = dir.join("meta.json");
    let tmp = dir.join(".meta.json.tmp");
    let bytes = serde_json::to_vec(meta).map_err(|err| format!("graph evidence meta: {err}"))?;
    {
        let mut file = File::create(&tmp).map_err(|err| format!("graph evidence meta: {err}"))?;
        file.write_all(&bytes)
            .map_err(|err| format!("graph evidence meta: {err}"))?;
    }
    fs::rename(tmp, path).map_err(|err| format!("graph evidence meta: {err}"))
}

fn prune_unlocked(repo_root: &Path, meta: &mut StoreMeta) {
    while meta.keys.len() > ENTRY_LIMIT {
        let oldest = meta.keys.remove(0);
        let _ = fs::remove_file(entry_path(repo_root, &oldest));
        // Equality-checked legacy unlink: colliding-prefix peers may own the 16-hex path.
        let legacy = legacy_entry_path(repo_root, &oldest);
        if read_matching_items(&oldest, &legacy).is_some() {
            let _ = fs::remove_file(&legacy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colliding_keys() -> (String, String) {
        let prefix = "0123456789abcdef";
        let key_a = format!("{prefix}{}", "a".repeat(48));
        let key_b = format!("{prefix}{}", "b".repeat(48));
        assert_eq!(&key_a[..16], &key_b[..16]);
        assert_ne!(key_a, key_b);
        (key_a, key_b)
    }

    fn stub_item(tag: &str) -> GraphOrphanItem {
        GraphOrphanItem {
            file: format!("{tag}.rs"),
            unit_name: tag.into(),
            start_line: 1,
            end_line: 2,
        }
    }

    #[test]
    fn graph_evidence_does_not_cross_load_on_shared_16_hex_prefix() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (key_a, key_b) = colliding_keys();
        store_items(tmp.path(), &key_a, vec![stub_item("a")]).unwrap();
        store_items(tmp.path(), &key_b, vec![stub_item("b")]).unwrap();

        // Truncated shared path + equality-only would yield None for A after B
        // overwrites; require coexistence of both identities.
        let loaded_a = load_items(tmp.path(), &key_a).expect("key A must remain");
        assert_eq!(loaded_a[0].unit_name, "a");
        let loaded_b = load_items(tmp.path(), &key_b).expect("key B");
        assert_eq!(loaded_b[0].unit_name, "b");
    }

    #[test]
    fn evidence_present_hits_meta_without_requiring_full_item_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        let key = "a".repeat(64);
        store_items(tmp.path(), &key, vec![stub_item("a")]).unwrap();
        assert!(evidence_present(tmp.path(), &key));
        assert!(!evidence_present(tmp.path(), &"b".repeat(64)));
        let _ = fs::remove_file(entry_path(tmp.path(), &key));
        assert!(
            !evidence_present(tmp.path(), &key),
            "meta membership alone must not claim presence when the blob is gone"
        );
    }

    #[test]
    fn evidence_mutable_digest_moves_with_covered_not_with_unrelated_schema_noise() {
        let mut covered = BTreeMap::new();
        covered.insert("a.py".into(), BTreeSet::from([1u32]));
        let a = evidence_mutable_digest(&[], &covered, "cfg");
        covered.insert("b.py".into(), BTreeSet::from([1u32]));
        let b = evidence_mutable_digest(&[], &covered, "cfg");
        assert_ne!(a, b);
        assert_eq!(a, evidence_mutable_digest(&[], &{
            let mut again = BTreeMap::new();
            again.insert("a.py".into(), BTreeSet::from([1u32]));
            again
        }, "cfg"));
    }

    #[test]
    fn graph_evidence_tmp_paths_differ_for_colliding_16_hex_prefix() {
        let (key_a, key_b) = colliding_keys();
        let dir = PathBuf::from("/tmp/graph-evidence-probe");
        // Bind to the production helper used by store_items — a local format!(...)
        // copy would still PASS if store_items kept a truncated tmp path.
        let tmp_a = publish_tmp_path(&dir, &key_a);
        let tmp_b = publish_tmp_path(&dir, &key_b);
        assert_ne!(
            tmp_a, tmp_b,
            "concurrent writers must not share one truncated tmp path"
        );
        let legacy_shared = dir.join(format!(".{}.tmp", &key_a[..16]));
        assert_ne!(tmp_a, legacy_shared);
        assert!(
            tmp_a.file_name().unwrap().to_string_lossy().contains(&key_a),
            "production tmp must embed the full key, got {}",
            tmp_a.display()
        );
    }

    #[test]
    fn store_items_writes_full_key_entry_paths() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (key_a, key_b) = colliding_keys();
        store_items(tmp.path(), &key_a, vec![stub_item("a")]).unwrap();
        store_items(tmp.path(), &key_b, vec![stub_item("b")]).unwrap();
        let dir = store_dir(tmp.path());
        assert!(
            entry_path(tmp.path(), &key_a).is_file(),
            "store_items must publish the full-key entry path for A"
        );
        assert!(
            entry_path(tmp.path(), &key_b).is_file(),
            "store_items must publish the full-key entry path for B"
        );
        assert!(
            !dir.join(format!("{}.json", &key_a[..16])).is_file(),
            "must not leave a shared 16-hex entry path after full-key publishes"
        );
    }

    #[test]
    fn store_items_takes_exclusive_store_lock() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = store_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        // Hold the production lock path exclusively. store_items must flock the
        // same file — touching `lock` without lock_exclusive would not block.
        let held = lock_store(&dir).expect("hold exclusive lock");
        let repo = tmp.path().to_path_buf();
        let key = "d".repeat(64);
        let handle = std::thread::spawn(move || {
            store_items(&repo, &key, vec![stub_item("blocked")])
        });
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_millis(200) {
            if handle.is_finished() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !handle.is_finished(),
            "store_items must block on an already-held exclusive store lock \
             (a create-only lock file would not serialize)"
        );
        drop(held);
        handle
            .join()
            .expect("store_items thread")
            .expect("store_items after unlock");
        let loaded = load_items(tmp.path(), &"d".repeat(64)).expect("post-lock load");
        assert_eq!(loaded[0].unit_name, "blocked");
    }

    #[test]
    fn load_items_waits_on_held_exclusive_lock() {
        let tmp = tempfile::TempDir::new().unwrap();
        let key = "e".repeat(64);
        store_items(tmp.path(), &key, vec![stub_item("cached")]).unwrap();
        let dir = store_dir(tmp.path());
        let held = lock_store(&dir).expect("hold exclusive lock");
        let repo = tmp.path().to_path_buf();
        let key_clone = key.clone();
        let handle = std::thread::spawn(move || load_items(&repo, &key_clone));
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_millis(200) {
            if handle.is_finished() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !handle.is_finished(),
            "load_items must block on an already-held exclusive store lock so \
             dual-path reads cannot race store_items migrate"
        );
        drop(held);
        let loaded = handle.join().expect("load_items thread").expect("cached");
        assert_eq!(loaded[0].unit_name, "cached");
    }

    #[test]
    fn load_items_reuses_legacy_16_hex_file_with_key_equality() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (key_a, key_b) = colliding_keys();
        let dir = store_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        // Simulate a pre-fix truncated publish for key A only.
        let legacy = StoredGraph {
            schema: SCHEMA.into(),
            key: key_a.clone(),
            items: vec![stub_item("legacy-a")],
        };
        let bytes = serde_json::to_vec(&legacy).unwrap();
        fs::write(legacy_entry_path(tmp.path(), &key_a), bytes).unwrap();
        assert!(
            !entry_path(tmp.path(), &key_a).exists(),
            "fixture must be legacy-only"
        );

        let loaded = load_items(tmp.path(), &key_a).expect("legacy A must load");
        assert_eq!(loaded[0].unit_name, "legacy-a");
        assert!(
            load_items(tmp.path(), &key_b).is_none(),
            "colliding-prefix peer B must not receive A's legacy payload"
        );

        // Full-key path must win when both filenames exist for the same identity.
        let full = StoredGraph {
            schema: SCHEMA.into(),
            key: key_a.clone(),
            items: vec![stub_item("full-a")],
        };
        fs::write(entry_path(tmp.path(), &key_a), serde_json::to_vec(&full).unwrap()).unwrap();
        assert_eq!(
            load_items(tmp.path(), &key_a).unwrap()[0].unit_name,
            "full-a",
            "full-key entry must be preferred over legacy"
        );

        // Publishing colliding-prefix B must not delete peer A's legacy file.
        // (Rewrite legacy-only again after removing full-key to isolate peer publish.)
        fs::remove_file(entry_path(tmp.path(), &key_a)).unwrap();
        fs::write(
            legacy_entry_path(tmp.path(), &key_a),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        store_items(tmp.path(), &key_b, vec![stub_item("b")]).unwrap();
        assert!(
            legacy_entry_path(tmp.path(), &key_a).is_file(),
            "publishing B must not remove peer A's legacy prefix file"
        );
        assert_eq!(
            load_items(tmp.path(), &key_a).unwrap()[0].unit_name,
            "legacy-a"
        );
        assert_eq!(load_items(tmp.path(), &key_b).unwrap()[0].unit_name, "b");

        // Republish migrates A to full-key path and removes matching legacy file.
        store_items(tmp.path(), &key_a, vec![stub_item("migrated-a")]).unwrap();
        assert!(entry_path(tmp.path(), &key_a).is_file());
        assert!(
            !legacy_entry_path(tmp.path(), &key_a).exists(),
            "matching legacy file must be removed after full-key publish"
        );
        assert_eq!(
            load_items(tmp.path(), &key_a).unwrap()[0].unit_name,
            "migrated-a"
        );
    }

    fn count_full_key_entries(repo_root: &Path) -> usize {
        let dir = store_dir(repo_root);
        fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                let name = e.file_name();
                let s = name.to_string_lossy();
                s.ends_with(".json") && s != "meta.json" && !s.starts_with('.') && s.len() > 20
            })
            .count()
    }

    #[test]
    fn store_items_prunes_above_entry_limit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let first = format!("{:064x}", 0u64);
        store_items(tmp.path(), &first, vec![stub_item("oldest")]).unwrap();
        for i in 1..(ENTRY_LIMIT + 3) {
            let key = format!("{i:064x}");
            store_items(tmp.path(), &key, vec![stub_item(&format!("k{i}"))]).unwrap();
        }
        let count = count_full_key_entries(tmp.path());
        assert!(
            count <= ENTRY_LIMIT,
            "graph evidence must prune above {ENTRY_LIMIT}, got {count}"
        );
        assert!(
            !entry_path(tmp.path(), &first).exists(),
            "oldest key must be reclaimed"
        );
        let newest = format!("{:064x}", ENTRY_LIMIT as u64 + 2);
        assert!(
            entry_path(tmp.path(), &newest).is_file(),
            "newest published key must survive prune"
        );
        assert_eq!(
            load_items(tmp.path(), &newest).unwrap()[0].unit_name,
            format!("k{}", ENTRY_LIMIT + 2)
        );
    }

    #[test]
    fn prune_removes_matching_legacy_entry_not_peer() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (key_a, key_b) = colliding_keys();
        let dir = store_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();

        // Full-key + legacy for A (same identity).
        let payload_a = StoredGraph {
            schema: SCHEMA.into(),
            key: key_a.clone(),
            items: vec![stub_item("a")],
        };
        fs::write(
            entry_path(tmp.path(), &key_a),
            serde_json::to_vec(&payload_a).unwrap(),
        )
        .unwrap();
        fs::write(
            legacy_entry_path(tmp.path(), &key_a),
            serde_json::to_vec(&payload_a).unwrap(),
        )
        .unwrap();

        // Peer B only on the shared 16-hex path would be wrong for this fixture:
        // A owns that legacy file. Give B a full-key entry so it survives prune(A).
        let payload_b = StoredGraph {
            schema: SCHEMA.into(),
            key: key_b.clone(),
            items: vec![stub_item("b")],
        };
        fs::write(
            entry_path(tmp.path(), &key_b),
            serde_json::to_vec(&payload_b).unwrap(),
        )
        .unwrap();

        let mut keys = vec![key_a.clone(), key_b.clone()];
        keys.extend((0..ENTRY_LIMIT - 1).map(|i| format!("{i:064x}")));
        assert_eq!(keys.len(), ENTRY_LIMIT + 1);
        let mut meta = StoreMeta { keys };
        prune_unlocked(tmp.path(), &mut meta);

        assert!(
            !entry_path(tmp.path(), &key_a).exists(),
            "pruned A full-key entry must be gone"
        );
        assert!(
            !legacy_entry_path(tmp.path(), &key_a).exists(),
            "prune must remove matching legacy truncated entry for A"
        );
        assert_eq!(
            load_items(tmp.path(), &key_b).unwrap()[0].unit_name,
            "b",
            "peer B must survive prune(A)"
        );
    }
}
