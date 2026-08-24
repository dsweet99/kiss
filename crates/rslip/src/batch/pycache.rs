use std::fs;
use std::path::Path;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    ".venv",
    "venv",
    "node_modules",
    ".kiss",
    ".pytest_cache",
];

pub(super) fn purge_pycache_under(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "__pycache__" {
            let _ = fs::remove_dir_all(&path);
            continue;
        }
        if SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        purge_pycache_under(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::purge_pycache_under;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn purge_removes_pycache_and_skips_dot_kiss() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let kept = root.join(".kiss").join("__pycache__");
        fs::create_dir_all(&kept).unwrap();
        fs::write(kept.join("runtime.pyc"), b"keep").unwrap();
        let gone = root.join("__pycache__");
        fs::create_dir_all(&gone).unwrap();
        fs::write(gone.join("test.pyc"), b"drop").unwrap();
        let nested = root.join("pkg").join("__pycache__");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("mod.pyc"), b"drop").unwrap();

        purge_pycache_under(root);

        assert!(!gone.exists());
        assert!(!nested.exists());
        assert!(kept.join("runtime.pyc").is_file());
    }
}
