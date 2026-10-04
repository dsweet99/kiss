use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use kiss::kiss_publication_barrier::unique_process_suffix;

pub(crate) struct PinGuard {
    path: PathBuf,
}

// Pins only need to be visible to concurrent GC on this host; a pin left by a crash is
// stale anyway, so the write is not fsynced.
fn write_pin(cache_root: &Path, kind: &str, generation_id: &str) -> Result<PinGuard, String> {
    let dir = cache_root.join(kind);
    fs::create_dir_all(&dir).map_err(|err| format!("error: kiss: create {kind} dir: {err}"))?;
    let path = dir.join(format!(
        "{}-{}.pin",
        std::process::id(),
        unique_process_suffix()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|err| format!("error: kiss: create_new {}: {err}", path.display()))?;
    file.write_all(format!("{generation_id}\n").as_bytes())
        .map_err(|err| format!("error: kiss: write {}: {err}", path.display()))?;
    Ok(PinGuard { path })
}

pub(crate) fn acquire_reader_pin(
    cache_root: &Path,
    generation_id: &str,
) -> Result<PinGuard, String> {
    write_pin(cache_root, "reader_pins", generation_id)
}

pub(crate) fn acquire_pending_pin(
    cache_root: &Path,
    generation_id: &str,
) -> Result<PinGuard, String> {
    write_pin(cache_root, "pending_pins", generation_id)
}

impl Drop for PinGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn pinned_ids_in(cache_root: &Path, kind: &str) -> Vec<String> {
    let dir = cache_root.join(kind);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for entry in entries.flatten() {
        if let Ok(bytes) = fs::read(entry.path())
            && let Ok(text) = std::str::from_utf8(&bytes)
        {
            let id = text.trim();
            if !id.is_empty() {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

pub(crate) fn pinned_generation_ids(cache_root: &Path) -> Vec<String> {
    let mut ids = pinned_ids_in(cache_root, "reader_pins");
    ids.extend(pinned_ids_in(cache_root, "pending_pins"));
    ids
}
