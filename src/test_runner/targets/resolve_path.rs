use std::path::{Path, PathBuf};

use super::super::parse::ParsedTestTarget;

pub(super) fn canonicalize_target_path(
    repo_root: &Path,
    parsed: &ParsedTestTarget,
) -> Result<PathBuf, String> {
    let candidate = if parsed.path.is_absolute() {
        parsed.path.clone()
    } else {
        repo_root.join(&parsed.path)
    };
    let abs = symlink_leaf_or_canonical(&candidate).map_err(|_| {
        format!(
            "target '{}': file not found at {}",
            parsed.raw,
            candidate.display()
        )
    })?;
    if !abs.is_file() {
        return Err(format!(
            "target '{}': {} is not a regular file",
            parsed.raw,
            abs.display()
        ));
    }
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    if !abs.starts_with(&root) {
        return Err(format!(
            "target '{}': path escapes repository root",
            parsed.raw
        ));
    }
    Ok(abs)
}

fn symlink_leaf_or_canonical(candidate: &Path) -> std::io::Result<PathBuf> {
    let meta = candidate.symlink_metadata()?;
    if meta.file_type().is_symlink() && candidate.is_file() {
        let name = candidate.file_name().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "symlink has no name")
        })?;
        let parent = candidate
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        return Ok(parent.canonicalize()?.join(name));
    }
    candidate.canonicalize()
}
