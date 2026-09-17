use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

use super::WATCH_SOCKET_TMP_DIR;

pub(crate) fn reclaim_stale_watch_sockets(keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(WATCH_SOCKET_TMP_DIR) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("sock") {
            continue;
        }
        if keep.is_some_and(|keep| keep == path.as_path()) {
            continue;
        }
        if watch_socket_is_live(&path) {
            continue;
        }
        let _ = std::fs::remove_file(&path);
    }
}

fn watch_socket_is_live(path: &Path) -> bool {
    match UnixStream::connect(path) {
        Ok(stream) => {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            true
        }
        Err(err) => !matches!(
            err.kind(),
            io::ErrorKind::ConnectionRefused
                | io::ErrorKind::NotFound
                | io::ErrorKind::PermissionDenied
        ),
    }
}
