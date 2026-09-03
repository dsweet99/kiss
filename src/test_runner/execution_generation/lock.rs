use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, Instant};

use fs2::FileExt;

pub(crate) struct PublicationLock {
    _file: File,
}

pub(crate) fn publication_lock(cache_root: &Path) -> Result<PublicationLock, String> {
    publication_lock_for(cache_root, Duration::from_secs(30))
}

fn publication_lock_for(cache_root: &Path, timeout: Duration) -> Result<PublicationLock, String> {
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
    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(PublicationLock { _file: file }),
            Err(err) if err.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                return Err(format!(
                    "error: kiss: generation publication lock timed out after {}ms",
                    timeout.as_millis()
                ));
            }
            Err(err) => {
                return Err(format!("error: kiss: generation publication lock: {err}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_lock_wait_is_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let _held = publication_lock_for(tmp.path(), Duration::from_millis(50)).unwrap();
        let started = Instant::now();
        let result = publication_lock_for(tmp.path(), Duration::from_millis(75));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(result.err().unwrap().contains("timed out"));
    }
}
