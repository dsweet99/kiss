//! File discovery and traversal

use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// Finds all Python files under the given root directory.
/// Respects .gitignore rules automatically.
pub fn find_python_files(root: &Path) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .hidden(false) // Don't skip hidden files (let gitignore handle it)
        .git_ignore(true) // Respect .gitignore
        .git_global(true) // Respect global gitignore
        .git_exclude(true) // Respect .git/info/exclude
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "py")
                .unwrap_or(false)
        })
        .map(|entry| entry.into_path())
        .collect()
}

