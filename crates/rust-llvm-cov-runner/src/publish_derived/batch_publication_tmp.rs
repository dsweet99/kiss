//! Sweep orphaned publication `*.tmp` files left by crashed publishers.

use std::fs;
use std::io;
use std::path::Path;

/// Remove leftover `*.tmp` files under `cache_root` (recursive).
///
/// Call only from selector-entry derived publishers (`publish_derived_state*`)
/// while holding the batch lock. Do not call from `kiss check` / check-aggregate
/// paths: crash-recovery QA starts a concurrent check reader that must leave a
/// killed writer's staged entry temp intact for the harness to observe.
pub(crate) fn sweep_orphaned_publication_tmps(cache_root: &Path) -> io::Result<()> {
    sweep_dir(cache_root)
}

fn sweep_dir(dir: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            sweep_dir(&path)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("tmp") {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "batch_publication_tmp_test.rs"]
mod tests;
