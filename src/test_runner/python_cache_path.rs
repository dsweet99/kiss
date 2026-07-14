use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn python_rslip_cache_root(repo_root: &Path) -> Result<PathBuf, String> {
    let host = linux_machine_id_host_component(Path::new("/etc/machine-id"))?;
    Ok(host_scoped_python_rslip_cache_root(
        &canonical_python_repo_root(repo_root)?,
        &host,
    ))
}

fn linux_machine_id_host_component(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| {
        format!(
            "error: kiss test: failed to read Linux machine id from {}: {err}",
            path.display()
        )
    })?;
    let trimmed = trim_ascii_whitespace(&bytes);
    if trimmed.is_empty() {
        return Err(format!(
            "error: kiss test: Linux machine id at {} is empty",
            path.display()
        ));
    }
    Ok(hex_encode_bytes(trimmed))
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

fn host_scoped_python_rslip_cache_root(repo_root: &Path, host_component: &str) -> PathBuf {
    repo_root
        .join(".kiss")
        .join("rslip_cache")
        .join("hosts")
        .join(host_component)
}

fn canonical_python_repo_root(repo_root: &Path) -> Result<PathBuf, String> {
    repo_root.canonicalize().map_err(|err| {
        format!(
            "error: kiss test: failed to canonicalize repository root {}: {err}",
            repo_root.display()
        )
    })
}

fn hex_encode_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
pub(crate) fn python_rslip_cache_root_for_machine_id(
    repo_root: &Path,
    machine_id: &[u8],
) -> Result<PathBuf, String> {
    let tmp = tempfile::NamedTempFile::new().map_err(|err| err.to_string())?;
    fs::write(tmp.path(), machine_id).map_err(|err| err.to_string())?;
    let host = linux_machine_id_host_component(tmp.path())?;
    Ok(host_scoped_python_rslip_cache_root(
        &canonical_python_repo_root(repo_root)?,
        &host,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_machine_id_and_repo_produce_same_host_cache_root() {
        let tmp = tempfile::tempdir().unwrap();
        let first = python_rslip_cache_root_for_machine_id(tmp.path(), b"machine-a\n").unwrap();
        let second = python_rslip_cache_root_for_machine_id(tmp.path(), b"machine-a").unwrap();

        assert_eq!(first, second);
        assert!(first.ends_with("hosts/6d616368696e652d61"));
    }

    #[test]
    fn different_machine_ids_produce_different_host_cache_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let first = python_rslip_cache_root_for_machine_id(tmp.path(), b"machine-a").unwrap();
        let second = python_rslip_cache_root_for_machine_id(tmp.path(), b"machine-b").unwrap();

        assert_ne!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_repo_paths_produce_same_host_cache_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let link = tmp.path().join("repo-link");
        fs::create_dir(&repo).unwrap();
        std::os::unix::fs::symlink(&repo, &link).unwrap();

        let direct = python_rslip_cache_root_for_machine_id(&repo, b"machine-a").unwrap();
        let symlinked = python_rslip_cache_root_for_machine_id(&link, b"machine-a").unwrap();

        assert_eq!(direct, symlinked);
        assert!(direct.starts_with(repo.canonicalize().unwrap()));
    }

    #[test]
    fn host_component_is_hex_only_without_separators() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), b"abc-123\n").unwrap();
        let component = linux_machine_id_host_component(tmp.path()).unwrap();

        assert!(component.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert!(!component.contains('/'));
        assert!(!component.contains('\\'));
    }

    #[test]
    fn blank_machine_id_is_rejected() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), b" \n\t").unwrap();

        assert!(linux_machine_id_host_component(tmp.path()).is_err());
        assert!(linux_machine_id_host_component(Path::new("/path/does/not/exist")).is_err());
    }
}
