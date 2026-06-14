use std::fs;
use std::path::Path;

use crate::util::db_path;
use crate::{Database, SCHEMA_VERSION};

pub fn load_database(repo_root: &Path) -> Result<Option<Database>, String> {
    let path = db_path(repo_root);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let header: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    let schema = header
        .get("schema_version")
        .and_then(serde_json::Value::as_u64);
    if schema != Some(u64::from(SCHEMA_VERSION)) {
        return Ok(None);
    }
    let db: Database = serde_json::from_slice(&bytes)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    Ok(Some(db))
}

pub fn write_database_atomic(repo_root: &Path, db: &Database) -> Result<(), String> {
    let path = db_path(repo_root);
    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid database path {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    let tmp = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec_pretty(db).map_err(|e| format!("failed to encode rslip db: {e}"))?;
    fs::write(&tmp, bytes).map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("failed to replace {}: {e}", path.display()))
}
