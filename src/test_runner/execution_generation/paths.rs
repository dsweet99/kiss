use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use kiss::kiss_publication_barrier::unique_process_suffix;

pub(crate) fn generations_dir(cache_root: &Path) -> PathBuf {
    cache_root.join("generations")
}

pub(crate) fn generation_dir(cache_root: &Path, generation_id: &str) -> PathBuf {
    generations_dir(cache_root).join(generation_id)
}

pub(crate) fn pointer_file(cache_root: &Path, name: &str) -> PathBuf {
    cache_root.join(name)
}

pub(crate) fn create_staging_dir(cache_root: &Path) -> Result<PathBuf, String> {
    let parent = generations_dir(cache_root);
    fs::create_dir_all(&parent).map_err(|err| err.to_string())?;
    let staged = parent.join(format!(".staging.{}", unique_process_suffix()));
    if staged.exists() {
        return Err(format!(
            "error: kiss: generation staging collision at {}",
            staged.display()
        ));
    }
    fs::create_dir_all(&staged).map_err(|err| err.to_string())?;
    Ok(staged)
}

pub(crate) fn write_create_new_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| format!("error: kiss: create_new {}: {err}", path.display()))?;
    file.write_all(bytes)
        .map_err(|err| format!("error: kiss: write {}: {err}", path.display()))?;
    file.sync_all()
        .map_err(|err| format!("error: kiss: sync_all {}: {err}", path.display()))?;
    Ok(())
}

pub(crate) fn sync_dir(path: &Path) -> Result<(), String> {
    let file = File::open(path)
        .map_err(|err| format!("error: kiss: open dir {}: {err}", path.display()))?;
    file.sync_all()
        .map_err(|err| format!("error: kiss: sync dir {}: {err}", path.display()))?;
    Ok(())
}
