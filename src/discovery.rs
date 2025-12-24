//! File discovery and traversal

use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// Supported source file languages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    Rust,
}

impl Language {
    /// Detect language from file extension
    pub fn from_path(path: &Path) -> Option<Language> {
        path.extension().and_then(|ext| {
            if ext == "py" {
                Some(Language::Python)
            } else if ext == "rs" {
                Some(Language::Rust)
            } else {
                None
            }
        })
    }

    /// Get the file extension for this language
    pub fn extension(&self) -> &'static str {
        match self {
            Language::Python => "py",
            Language::Rust => "rs",
        }
    }
}

/// A discovered source file with its language
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub language: Language,
}

/// Finds all Python files under the given root directory.
/// Respects .gitignore rules automatically.
pub fn find_python_files(root: &Path) -> Vec<PathBuf> {
    find_files_by_extension(root, "py")
}

/// Finds all Rust files under the given root directory.
/// Respects .gitignore rules automatically.
pub fn find_rust_files(root: &Path) -> Vec<PathBuf> {
    find_files_by_extension(root, "rs")
}

/// Finds all source files (Python and Rust) under the given root directory.
/// Respects .gitignore rules automatically.
pub fn find_source_files(root: &Path) -> Vec<SourceFile> {
    WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .filter_map(|entry| {
            let path = entry.into_path();
            Language::from_path(&path).map(|language| SourceFile { path, language })
        })
        .collect()
}

fn find_files_by_extension(root: &Path, ext: &str) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .filter(move |entry| {
            entry
                .path()
                .extension()
                .map(|e| e == ext)
                .unwrap_or(false)
        })
        .map(|entry| entry.into_path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_language_from_path_python() {
        assert_eq!(Language::from_path(Path::new("foo.py")), Some(Language::Python));
    }

    #[test]
    fn test_language_from_path_rust() {
        assert_eq!(Language::from_path(Path::new("bar.rs")), Some(Language::Rust));
    }

    #[test]
    fn test_language_from_path_unknown() {
        assert_eq!(Language::from_path(Path::new("file.txt")), None);
        assert_eq!(Language::from_path(Path::new("no_extension")), None);
    }

    #[test]
    fn test_language_extension() {
        assert_eq!(Language::Python.extension(), "py");
        assert_eq!(Language::Rust.extension(), "rs");
    }

    #[test]
    fn test_language_eq() {
        assert_eq!(Language::Python, Language::Python);
        assert_ne!(Language::Python, Language::Rust);
    }

    #[test]
    fn test_source_file_struct() {
        let sf = SourceFile { path: PathBuf::from("test.py"), language: Language::Python };
        assert_eq!(sf.path, PathBuf::from("test.py"));
        assert_eq!(sf.language, Language::Python);
    }

    #[test]
    fn test_find_python_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.py"), "").unwrap();
        fs::write(tmp.path().join("b.rs"), "").unwrap();
        fs::write(tmp.path().join("c.txt"), "").unwrap();
        let files = find_python_files(tmp.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("a.py"));
    }

    #[test]
    fn test_find_rust_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.py"), "").unwrap();
        fs::write(tmp.path().join("b.rs"), "").unwrap();
        let files = find_rust_files(tmp.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("b.rs"));
    }

    #[test]
    fn test_find_source_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.py"), "").unwrap();
        fs::write(tmp.path().join("b.rs"), "").unwrap();
        fs::write(tmp.path().join("c.txt"), "").unwrap();
        let files = find_source_files(tmp.path());
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_find_files_by_extension() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("x.py"), "").unwrap();
        fs::write(tmp.path().join("y.py"), "").unwrap();
        let files = find_files_by_extension(tmp.path(), "py");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_find_files_empty_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(find_python_files(tmp.path()).is_empty());
    }

    #[test]
    fn test_find_files_nested() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("nested.py"), "").unwrap();
        let files = find_python_files(tmp.path());
        assert_eq!(files.len(), 1);
    }
}

