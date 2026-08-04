use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

use crate::{after_rename, after_sync_before_rename};

/// Atomically publish `temporary_path` to `final_path` with QA barrier hooks.
///
/// Order: same-parent check → `create_dir_all` → `create_new` tmp → `write` →
/// `sync_all` → `after_sync_before_rename` → drop handle → `rename` (best-effort
/// tmp remove on rename Err) → `after_rename` → parent-dir `sync_all`.
pub fn publish_atomically(
    artifact: &str,
    final_path: &Path,
    temporary_path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let final_parent = final_path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "final_path has no parent directory")
    })?;
    let temporary_parent = temporary_path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "temporary_path has no parent directory",
        )
    })?;
    if temporary_parent != final_parent {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "temporary_path and final_path must share the same parent directory",
        ));
    }

    fs::create_dir_all(final_parent)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary_path)?;
    write(&mut file)?;
    file.sync_all()?;
    after_sync_before_rename(artifact, temporary_path, final_path)?;
    drop(file);
    if let Err(err) = fs::rename(temporary_path, final_path) {
        let _ = fs::remove_file(temporary_path);
        return Err(err);
    }
    after_rename(artifact, temporary_path, final_path)?;
    let parent_dir = File::open(final_parent)?;
    parent_dir.sync_all()?;
    Ok(())
}
