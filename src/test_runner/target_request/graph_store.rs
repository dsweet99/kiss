use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::digest::digest_bytes;

const SCHEMA: &str = "graph-evidence-v3";

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

pub(crate) fn load_items(repo_root: &Path, key: &str) -> Option<Vec<GraphOrphanItem>> {
    let stored: StoredGraph =
        serde_json::from_slice(&fs::read(entry_path(repo_root, key)).ok()?).ok()?;
    (stored.schema == SCHEMA && stored.key == key).then_some(stored.items)
}

pub(crate) fn store_items(
    repo_root: &Path,
    key: &str,
    items: Vec<GraphOrphanItem>,
) -> Result<(), String> {
    let dir = store_dir(repo_root);
    fs::create_dir_all(&dir).map_err(|err| format!("graph evidence store: {err}"))?;
    let stored = StoredGraph {
        schema: SCHEMA.into(),
        key: key.into(),
        items,
    };
    let path = entry_path(repo_root, key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("graph evidence store: {err}"))?;
    }
    let tmp = dir.join(format!(".{}.tmp", &key[..16.min(key.len())]));
    let bytes = serde_json::to_vec(&stored).map_err(|err| format!("graph evidence json: {err}"))?;
    {
        let mut file = File::create(&tmp).map_err(|err| format!("graph evidence write: {err}"))?;
        file.write_all(&bytes)
            .map_err(|err| format!("graph evidence write: {err}"))?;
    }
    fs::rename(tmp, path).map_err(|err| format!("graph evidence publish: {err}"))
}

fn store_dir(repo_root: &Path) -> PathBuf {
    repo_root
        .join("target")
        .join("kiss-plan")
        .join("graph-evidence")
}

fn entry_path(repo_root: &Path, key: &str) -> PathBuf {
    store_dir(repo_root).join(format!("{}.json", &key[..16.min(key.len())]))
}
