use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::digest::digest_bytes;

const SCHEMA: &str = "graph-evidence-v4";
/// Cap retained evidence keys.
const ENTRY_LIMIT: usize = 64;

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
    config: &str,
) -> String {
    let mut files = Vec::new();
    for path in py.iter().chain(rs) {
        let rel = path
            .strip_prefix(repo_root)
            .map(|item| item.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string_lossy().into_owned());
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
        "config": config,
    });
    digest_bytes(&serde_json::to_vec(&payload).expect("graph evidence key"))
}

/// Config (+ orphan_allowed) half of `evidence_key`, without source bytes.
/// After a worktree match, sources are already validated; ready freshness uses this.
pub(crate) fn evidence_mutable_digest(orphan_allowed: &[String], config: &str) -> String {
    let payload = serde_json::json!({
        "schema": SCHEMA,
        "orphan_allowed": orphan_allowed,
        "config": config,
    });
    digest_bytes(&serde_json::to_vec(&payload).expect("graph evidence mutable"))
}

pub(crate) fn load_items(repo_root: &Path, key: &str) -> Option<Vec<GraphOrphanItem>> {
    let dir = store_dir(repo_root);
    if !dir.is_dir() {
        return None;
    }
    let _lock = lock_store(&dir).ok()?;
    read_matching_items(key, &entry_path(repo_root, key))
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
    // Serialize concurrent kiss test publishers (VISION: multi-process, no corruption).
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
    // concurrent kiss test assembly writers.
    let tmp = publish_tmp_path(&dir, key);
    let bytes = serde_json::to_vec(&stored).map_err(|err| format!("graph evidence json: {err}"))?;
    {
        let mut file = File::create(&tmp).map_err(|err| format!("graph evidence write: {err}"))?;
        file.write_all(&bytes)
            .map_err(|err| format!("graph evidence write: {err}"))?;
    }
    fs::rename(tmp, path).map_err(|err| format!("graph evidence publish: {err}"))?;
    // Track published keys and prune oldest above ENTRY_LIMIT (kt_bug.md retention).
    let mut meta = read_meta(&dir);
    meta.keys.retain(|item| item != key);
    meta.keys.push(key.to_string());
    prune_unlocked(repo_root, &mut meta);
    write_meta(&dir, &meta)?;
    Ok(())
}

fn store_dir(repo_root: &Path) -> PathBuf {
    crate::test_runner::test_state_dir(repo_root).join("graph-evidence")
}

fn entry_path(repo_root: &Path, key: &str) -> PathBuf {
    store_dir(repo_root).join(format!("{key}.json"))
}

fn publish_tmp_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!(".{key}.tmp"))
}

fn lock_store(dir: &Path) -> Result<kiss::test_state_lock::TestStateLock, String> {
    let state_dir = dir.parent().unwrap_or(dir);
    crate::test_runner::lock_test_state(state_dir)
        .map_err(|err| format!("graph evidence lock: {err}"))
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
    fn evidence_mutable_digest_moves_with_config_and_orphan_allowed() {
        let base = evidence_mutable_digest(&[], "cfg");
        assert_eq!(base, evidence_mutable_digest(&[], "cfg"));
        assert_ne!(base, evidence_mutable_digest(&[], "cfg2"));
        assert_ne!(base, evidence_mutable_digest(&["x".into()], "cfg"));
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
        let truncated_shared = dir.join(format!(".{}.tmp", &key_a[..16]));
        assert_ne!(tmp_a, truncated_shared);
        assert!(
            tmp_a
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(&key_a),
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
        let handle =
            std::thread::spawn(move || store_items(&repo, &key, vec![stub_item("blocked")]));
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
             reads cannot race store_items"
        );
        drop(held);
        let loaded = handle.join().expect("load_items thread").expect("cached");
        assert_eq!(loaded[0].unit_name, "cached");
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
}
