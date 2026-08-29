use std::fs::{File, OpenOptions};
use std::path::Path;

use fs2::FileExt;

pub(crate) struct PublicationLock {
    _file: File,
}

pub(crate) fn publication_lock(cache_root: &Path) -> Result<PublicationLock, String> {
    let path = cache_root.join("publication.lock");
    let parent = path
        .parent()
        .ok_or_else(|| "error: kiss: generation lock path has no parent".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|err| format!("error: kiss: create generation lock dir: {err}"))?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|err| format!("error: kiss: open generation lock: {err}"))?;
    file.lock_exclusive()
        .map_err(|err| format!("error: kiss: generation publication lock: {err}"))?;
    Ok(PublicationLock { _file: file })
}
