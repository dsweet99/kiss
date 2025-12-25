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
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension().and_then(|ext| {
            if ext == "py" {
                Some(Self::Python)
            } else if ext == "rs" {
                Some(Self::Rust)
            } else {
                None
            }
        })
    }

    /// Get the file extension for this language
    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::Rust => "rs",
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
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
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
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
        .filter(move |entry| {
            entry
                .path()
                .extension()
                .is_some_and(|e| e == ext)
        })
        .map(ignore::DirEntry::into_path)
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

    // --- Design doc: File Filtering and .gitignore ---
    // "Respect .gitignore"
    
    /// Helper to create .git directory; skips test if permission denied (e.g., in sandbox)
    fn try_create_git_dir(path: &std::path::Path) -> bool {
        fs::create_dir(path.join(".git")).is_ok()
    }

    #[test]
    fn test_gitignore_excludes_ignored_files() {
        let tmp = TempDir::new().unwrap();
        
        // Need to initialize git for gitignore to work with the `ignore` crate
        if !try_create_git_dir(tmp.path()) {
            eprintln!("Skipping test_gitignore_excludes_ignored_files: cannot create .git (sandbox?)");
            return;
        }
        
        // Create .gitignore that ignores specific file
        fs::write(tmp.path().join(".gitignore"), "ignored.py\n").unwrap();
        
        // Create both ignored and included files
        fs::write(tmp.path().join("ignored.py"), "# should be ignored").unwrap();
        fs::write(tmp.path().join("included.py"), "# should be included").unwrap();
        
        let files = find_python_files(tmp.path());
        
        let filenames: Vec<String> = files.iter()
            .filter_map(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .collect();
        
        assert!(filenames.contains(&"included.py".to_string()), 
            "included.py should be found");
        assert!(!filenames.contains(&"ignored.py".to_string()), 
            "ignored.py should be excluded by .gitignore");
    }

    #[test]
    fn test_gitignore_excludes_directory_patterns() {
        let tmp = TempDir::new().unwrap();
        
        if !try_create_git_dir(tmp.path()) {
            eprintln!("Skipping test_gitignore_excludes_directory_patterns: cannot create .git (sandbox?)");
            return;
        }
        
        // Create .gitignore that ignores __pycache__ directory
        fs::write(tmp.path().join(".gitignore"), "__pycache__/\n").unwrap();
        
        // Create __pycache__ directory with files
        let cache_dir = tmp.path().join("__pycache__");
        fs::create_dir(&cache_dir).unwrap();
        fs::write(cache_dir.join("cached.py"), "# cached file").unwrap();
        
        // Create normal file
        fs::write(tmp.path().join("normal.py"), "# normal file").unwrap();
        
        let files = find_python_files(tmp.path());
        
        let filenames: Vec<String> = files.iter()
            .filter_map(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .collect();
        
        assert!(filenames.contains(&"normal.py".to_string()),
            "normal.py should be found");
        assert!(!filenames.contains(&"cached.py".to_string()),
            "__pycache__/cached.py should be excluded");
    }

    #[test]
    fn test_gitignore_glob_patterns() {
        let tmp = TempDir::new().unwrap();
        
        if !try_create_git_dir(tmp.path()) {
            eprintln!("Skipping test_gitignore_glob_patterns: cannot create .git (sandbox?)");
            return;
        }
        
        // Create .gitignore with glob pattern
        fs::write(tmp.path().join(".gitignore"), "*.generated.py\n").unwrap();
        
        // Create matching and non-matching files
        fs::write(tmp.path().join("code.generated.py"), "# generated").unwrap();
        fs::write(tmp.path().join("code.py"), "# normal").unwrap();
        
        let files = find_python_files(tmp.path());
        
        let filenames: Vec<String> = files.iter()
            .filter_map(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .collect();
        
        assert!(filenames.contains(&"code.py".to_string()),
            "code.py should be found");
        assert!(!filenames.contains(&"code.generated.py".to_string()),
            "*.generated.py should be excluded by glob pattern");
    }

    #[test]
    fn test_nested_gitignore() {
        let tmp = TempDir::new().unwrap();
        
        if !try_create_git_dir(tmp.path()) {
            eprintln!("Skipping test_nested_gitignore: cannot create .git (sandbox?)");
            return;
        }
        
        // Create subdirectory with its own .gitignore
        let subdir = tmp.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join(".gitignore"), "local_ignored.py\n").unwrap();
        
        // Create files
        fs::write(subdir.join("local_ignored.py"), "# ignored by nested gitignore").unwrap();
        fs::write(subdir.join("included.py"), "# included").unwrap();
        fs::write(tmp.path().join("root.py"), "# root file").unwrap();
        
        let files = find_python_files(tmp.path());
        
        let filenames: Vec<String> = files.iter()
            .filter_map(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .collect();
        
        assert!(filenames.contains(&"root.py".to_string()));
        assert!(filenames.contains(&"included.py".to_string()));
        assert!(!filenames.contains(&"local_ignored.py".to_string()),
            "file ignored by nested .gitignore should be excluded");
    }
}

