use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::util::{content_digest, normalize_path};
use crate::{FileRecord, FileRole, RSLIP_VERSION, SCHEMA_VERSION};

fn file_mtime_ns(meta: &fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|mtime| mtime.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn file_record(repo_root: &Path, path: &Path, role: FileRole) -> Result<FileRecord, String> {
    let bytes = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let meta = fs::metadata(path).map_err(|e| format!("failed to stat {}: {e}", path.display()))?;
    Ok(FileRecord {
        path: normalize_path(repo_root, path),
        role,
        content_digest: content_digest(&bytes),
        len: meta.len(),
        mtime_ns: file_mtime_ns(&meta),
        coverage: None,
    })
}

fn walk_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .add_custom_ignore_filename(".kissignore")
        .filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some(".git" | ".kiss" | "target" | "__pycache__")
            )
        })
        .build();
    for entry in walker {
        let entry = entry.map_err(|e| format!("failed to read directory entry: {e}"))?;
        let path = entry.path();
        if path.is_file() {
            out.push(path.to_path_buf());
        }
    }
    Ok(())
}

fn is_python(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
}

fn is_config(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            ".kissconfig"
                | ".kissignore"
                | "pyproject.toml"
                | "pytest.ini"
                | "tox.ini"
                | "setup.cfg"
        )
    )
}

pub(crate) fn is_test_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower == "conftest.py"
        || (lower.starts_with("test_") && lower.ends_with(".py"))
        || lower.ends_with("_test.py")
}

pub(crate) fn is_in_test_directory(path: &Path) -> bool {
    path.components().any(|component| {
        let s = component.as_os_str().to_string_lossy();
        s == "tests" || s == "test"
    })
}

pub(crate) fn classify_python(path: &Path) -> FileRole {
    if is_test_file(path) || is_in_test_directory(path) {
        FileRole::Test
    } else {
        FileRole::Source
    }
}

pub fn discover_repo_files(repo_root: &Path) -> Result<Vec<FileRecord>, String> {
    let mut paths = Vec::new();
    walk_files(repo_root, &mut paths)?;
    let mut records = Vec::new();
    for path in paths {
        if is_python(&path) {
            records.push(file_record(repo_root, &path, classify_python(&path))?);
        } else if is_config(&path) {
            records.push(file_record(repo_root, &path, FileRole::Config)?);
        }
    }
    records.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(records)
}

pub fn discover_tests(
    repo_root: &Path,
    files: &[FileRecord],
) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for file in files.iter().filter(|file| file.role == FileRole::Test) {
        let source = fs::read_to_string(repo_root.join(&file.path))
            .map_err(|e| format!("failed to read {}: {e}", file.path))?;
        let mut current_class: Option<String> = None;
        for line in source.lines() {
            let trimmed = line.trim_start();
            let indent = line.len().saturating_sub(trimmed.len());
            if indent == 0 && !trimmed.starts_with("class Test") {
                current_class = None;
            }
            if let Some(rest) = trimmed.strip_prefix("class Test") {
                let name = rest
                    .split(['(', ':'])
                    .next()
                    .map_or("Test", |tail| tail.trim());
                current_class = Some(format!("Test{name}"));
            }
            if let Some(rest) = trimmed.strip_prefix("def test_") {
                let name = format!("test_{}", rest.split(['(', ':']).next().unwrap_or_default());
                let id = current_class
                    .as_ref()
                    .filter(|_| indent > 0)
                    .map_or_else(|| name.clone(), |class| format!("{class}::{name}"));
                out.push((format!("{}::{id}", file.path), file.path.clone()));
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}
pub(crate) fn config_fingerprints(files: &[FileRecord]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::from([
        ("schema_version".to_string(), SCHEMA_VERSION.to_string()),
        ("rslip_version".to_string(), RSLIP_VERSION.to_string()),
    ]);
    for file in files.iter().filter(|file| file.role == FileRole::Config) {
        out.insert(file.path.clone(), file.content_digest.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_identifies_python_tests_sources_and_configs() {
        for path in [
            Path::new("test_api.py"),
            Path::new("api_test.py"),
            Path::new("conftest.py"),
        ] {
            assert!(
                is_test_file(path),
                "{} should be a test file",
                path.display()
            );
        }
        for path in [Path::new("api.py"), Path::new("test_api.rs")] {
            assert!(
                !is_test_file(path),
                "{} is not a Python test",
                path.display()
            );
        }

        for path in [
            Path::new("pkg/tests/api.py"),
            Path::new("pkg/test/helpers.py"),
        ] {
            assert!(is_in_test_directory(path));
        }
        assert!(!is_in_test_directory(Path::new("pkg/source/api.py")));
        assert_eq!(
            classify_python(Path::new("pkg/tests/api.py")),
            FileRole::Test
        );
        assert_eq!(classify_python(Path::new("pkg/api.py")), FileRole::Source);
        assert!(is_config(Path::new("pyproject.toml")));
    }
}
