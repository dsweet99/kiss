use std::fs;
use std::path::Path;

use super::paths::generations_dir;
use super::pin::pinned_generation_ids;
use super::pointer::read_pointer;

pub(crate) fn remove_staging_dirs(cache_root: &Path) {
    let Ok(entries) = fs::read_dir(generations_dir(cache_root)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".staging.") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

pub(crate) fn reclaim_unreferenced(cache_root: &Path) -> Result<(), String> {
    let pointer = read_pointer(cache_root)?;
    let mut keep = pinned_generation_ids(cache_root);
    if let Some(pointer) = pointer {
        keep.push(pointer.generation_id);
        if !pointer.parent_generation_id.is_empty() {
            keep.push(pointer.parent_generation_id);
        }
    }
    let Ok(entries) = fs::read_dir(generations_dir(cache_root)) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || keep.iter().any(|id| id == name.as_ref()) {
            continue;
        }
        let _ = fs::remove_dir_all(entry.path());
    }
    Ok(())
}
