use std::path::{Path, PathBuf};

pub(crate) fn repository_root_for_universe(universe: &Path) -> PathBuf {
    let start = universe
        .canonicalize()
        .unwrap_or_else(|_| universe.to_path_buf());
    let start_dir = if start.is_file() {
        start.parent().unwrap_or(&start).to_path_buf()
    } else {
        start.clone()
    };
    let mut cursor = start_dir.as_path();
    loop {
        if cursor.join(".git").exists() {
            return cursor.to_path_buf();
        }
        let Some(parent) = cursor.parent() else {
            return start_dir;
        };
        cursor = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::repository_root_for_universe;
    use std::fs;

    #[test]
    fn falls_back_to_canonical_universe_without_git() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        assert_eq!(
            repository_root_for_universe(&src),
            src.canonicalize().unwrap()
        );
    }

    #[test]
    fn falls_back_to_parent_for_file_without_git() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let file = src.join("lib.py");
        fs::create_dir_all(&src).unwrap();
        fs::write(&file, "VALUE = 1\n").unwrap();
        assert_eq!(
            repository_root_for_universe(&file),
            src.canonicalize().unwrap()
        );
    }

    #[test]
    fn walks_up_to_git_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("repo/src/pkg");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir(tmp.path().join("repo/.git")).unwrap();
        assert_eq!(
            repository_root_for_universe(&nested),
            tmp.path().join("repo").canonicalize().unwrap()
        );
    }
}
