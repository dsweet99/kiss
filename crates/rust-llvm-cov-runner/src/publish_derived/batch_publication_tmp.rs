
use crate::publish_derived::batch_io_skip_not_found::{
    dir_entry_ok_missing, file_type_ok_missing, read_dir_ok_missing,
};
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

const ORPHAN_TMP_MIN_AGE: Duration = Duration::from_secs(60);

pub(crate) fn sweep_orphaned_publication_tmps(cache_root: &Path) -> io::Result<()> {
    let cutoff = SystemTime::now()
        .checked_sub(ORPHAN_TMP_MIN_AGE)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    sweep_dir(cache_root, cutoff)
}

fn sweep_dir(dir: &Path, cutoff: SystemTime) -> io::Result<()> {
    let Some(entries) = read_dir_ok_missing(dir)? else {
        return Ok(());
    };
    for entry in entries {
        sweep_one(entry, cutoff)?;
    }
    Ok(())
}

fn sweep_one(entry: io::Result<fs::DirEntry>, cutoff: SystemTime) -> io::Result<()> {
    let Some(entry) = dir_entry_ok_missing(entry)? else {
        return Ok(());
    };
    let Some(file_type) = file_type_ok_missing(&entry)? else {
        return Ok(());
    };
    let path = entry.path();
    if file_type.is_dir() {
        return sweep_dir(&path, cutoff);
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("tmp") {
        return Ok(());
    }
    if tmp_mtime_older_than(&path, cutoff) {
        let _ = fs::remove_file(&path);
    }
    Ok(())
}

fn tmp_mtime_older_than(path: &Path, cutoff: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map(|modified| modified <= cutoff)
        .unwrap_or(true)
}

#[cfg(test)]
#[path = "batch_publication_tmp_test.rs"]
mod tests;
