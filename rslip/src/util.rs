use std::path::{Path, PathBuf};

pub fn db_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("rslip.json")
}

pub fn content_digest(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn normalize_path(repo_root: &Path, path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    abs.strip_prefix(repo_root)
        .unwrap_or(&abs)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_and_paths_are_stable() {
        for (left, right, should_match) in [
            (b"same".as_slice(), b"same".as_slice(), true),
            (b"same", b"some", false),
        ] {
            if should_match {
                assert_eq!(content_digest(left), content_digest(right));
            } else {
                assert_ne!(content_digest(left), content_digest(right));
            }
        }

        let repo = Path::new("/tmp/repo");
        let path = db_path(repo);
        let normalized_abs = normalize_path(repo, Path::new("/tmp/repo/pkg/app.py"));
        let normalized_rel = normalize_path(repo, Path::new("pkg/app.py"));
        assert_eq!(path, PathBuf::from("/tmp/repo/.kiss/rslip.json"));
        assert_eq!(normalized_abs, "pkg/app.py");
        assert_eq!(normalized_rel, "pkg/app.py");
    }
}
