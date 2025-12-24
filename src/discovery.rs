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

