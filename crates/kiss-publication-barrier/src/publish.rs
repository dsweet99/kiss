use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
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
    publish_atomically_inner(artifact, final_path, temporary_path, PublishSync::FileAndParent, write)
}

/// Same publish order as [`publish_atomically`], but skip per-file and parent
/// `sync_all`. Batch writers flush bytes, then fsync the directory once.
pub fn publish_atomically_without_parent_sync(
    artifact: &str,
    final_path: &Path,
    temporary_path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    publish_atomically_inner(artifact, final_path, temporary_path, PublishSync::FlushOnly, write)
}

#[derive(Clone, Copy)]
enum PublishSync {
    FileAndParent,
    FlushOnly,
}

fn publish_atomically_inner(
    artifact: &str,
    final_path: &Path,
    temporary_path: &Path,
    sync: PublishSync,
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

    fs::create_dir_all(final_parent).map_err(|err| {
        path_step_err(artifact, "create_dir_all", final_parent, err)
    })?;
    let mut file = open_publish_tmp(artifact, temporary_path, final_parent)?;
    write(&mut file).map_err(|err| path_step_err(artifact, "write", temporary_path, err))?;
    match sync {
        PublishSync::FileAndParent => file
            .sync_all()
            .map_err(|err| path_step_err(artifact, "sync_all", temporary_path, err))?,
        PublishSync::FlushOnly => file
            .flush()
            .map_err(|err| path_step_err(artifact, "flush", temporary_path, err))?,
    }
    after_sync_before_rename(artifact, temporary_path, final_path)
        .map_err(|err| step_err(artifact, "after_sync_before_rename", err))?;
    drop(file);
    if let Err(err) = fs::rename(temporary_path, final_path) {
        let _ = fs::remove_file(temporary_path);
        return Err(io::Error::new(
            err.kind(),
            format!(
                "publish_atomically[{artifact}] rename {} -> {}: {err}",
                temporary_path.display(),
                final_path.display()
            ),
        ));
    }
    after_rename(artifact, temporary_path, final_path)
        .map_err(|err| step_err(artifact, "after_rename", err))?;
    match sync {
        PublishSync::FileAndParent => sync_publish_parent(artifact, final_parent),
        PublishSync::FlushOnly => Ok(()),
    }
}

fn step_err(artifact: &str, step: &str, err: io::Error) -> io::Error {
    io::Error::new(
        err.kind(),
        format!("publish_atomically[{artifact}] {step}: {err}"),
    )
}

fn path_step_err(artifact: &str, step: &str, path: &Path, err: io::Error) -> io::Error {
    io::Error::new(
        err.kind(),
        format!(
            "publish_atomically[{artifact}] {step} {}: {err}",
            path.display()
        ),
    )
}

/// `create_new` the temp file; if the parent vanished, recreate and retry once.
pub(crate) fn open_publish_tmp(
    artifact: &str,
    temporary_path: &Path,
    final_parent: &Path,
) -> io::Result<File> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary_path)
    {
        Ok(file) => Ok(file),
        // Parent can vanish between create_dir_all and create_new under races.
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(final_parent)
                .map_err(|retry| path_step_err(artifact, "create_dir_all retry", final_parent, retry))?;
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(temporary_path)
                .map_err(|retry| path_step_err(artifact, "create_new retry", temporary_path, retry))
        }
        Err(err) => Err(path_step_err(artifact, "create_new", temporary_path, err)),
    }
}

/// Sync the parent directory after rename. Missing parent is success: the bytes
/// were already published, and a concurrent tree replacement can remove the dir.
pub(crate) fn sync_publish_parent(artifact: &str, final_parent: &Path) -> io::Result<()> {
    match File::open(final_parent) {
        Ok(parent_dir) => parent_dir
            .sync_all()
            .map_err(|err| path_step_err(artifact, "parent sync_all", final_parent, err)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(path_step_err(artifact, "sync parent", final_parent, err)),
    }
}
