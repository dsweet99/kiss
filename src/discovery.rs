use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    Rust,
}

impl Language {
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension().and_then(|ext| {
            if ext == "py" { Some(Self::Python) }
            else if ext == "rs" { Some(Self::Rust) }
            else { None }
        })
    }

    pub const fn extension(&self) -> &'static str {
        match self { Self::Python => "py", Self::Rust => "rs" }
    }
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub language: Language,
}

pub fn find_python_files(root: &Path) -> Vec<PathBuf> { find_files_by_extension(root, "py") }
pub fn find_rust_files(root: &Path) -> Vec<PathBuf> { find_files_by_extension(root, "rs") }

fn has_ignored_prefix(name: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| name.starts_with(prefix))
}

fn should_ignore(path: &Path, ignore_prefixes: &[String]) -> bool {
    path.components().any(|c| {
        c.as_os_str().to_str().is_some_and(|s| has_ignored_prefix(s, ignore_prefixes))
    })
}

pub fn find_source_files(root: &Path) -> Vec<SourceFile> {
    find_source_files_with_ignore(root, &[])
}

pub fn find_source_files_with_ignore(root: &Path, ignore_prefixes: &[String]) -> Vec<SourceFile> {
    WalkBuilder::new(root)
        .hidden(false).git_ignore(true).git_global(true).git_exclude(true).build()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
        .filter_map(|entry| {
            let path = entry.into_path();
            if should_ignore(&path, ignore_prefixes) { return None; }
            Language::from_path(&path).map(|language| SourceFile { path, language })
        })
        .collect()
}

fn find_files_by_extension(root: &Path, ext: &str) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .hidden(false).git_ignore(true).git_global(true).git_exclude(true).build()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
        .filter(move |entry| entry.path().extension().is_some_and(|e| e == ext))
        .map(ignore::DirEntry::into_path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_language_from_path() {
        assert_eq!(Language::from_path(Path::new("foo.py")), Some(Language::Python));
        assert_eq!(Language::from_path(Path::new("bar.rs")), Some(Language::Rust));
        assert_eq!(Language::from_path(Path::new("file.txt")), None);
    }

    #[test]
    fn test_language_extension() {
        assert_eq!(Language::Python.extension(), "py");
        assert_eq!(Language::Rust.extension(), "rs");
    }

    #[test]
    fn test_source_file_struct() {
        let sf = SourceFile { path: PathBuf::from("test.py"), language: Language::Python };
        assert_eq!(sf.language, Language::Python);
    }

    #[test]
    fn test_find_python_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.py"), "").unwrap();
        fs::write(tmp.path().join("b.rs"), "").unwrap();
        assert_eq!(find_python_files(tmp.path()).len(), 1);
    }

    #[test]
    fn test_find_rust_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.py"), "").unwrap();
        fs::write(tmp.path().join("b.rs"), "").unwrap();
        assert_eq!(find_rust_files(tmp.path()).len(), 1);
    }

    #[test]
    fn test_find_source_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.py"), "").unwrap();
        fs::write(tmp.path().join("b.rs"), "").unwrap();
        fs::write(tmp.path().join("c.txt"), "").unwrap();
        assert_eq!(find_source_files(tmp.path()).len(), 2);
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
        assert_eq!(find_python_files(tmp.path()).len(), 1);
    }

    #[test]
    fn test_should_ignore() {
        assert!(should_ignore(Path::new("tests/fake_python/foo.py"), &["fake_".to_string()]));
        assert!(should_ignore(Path::new("mock_data/test.rs"), &["mock_".to_string()]));
        assert!(!should_ignore(Path::new("src/main.rs"), &["fake_".to_string()]));
        assert!(!should_ignore(Path::new("tests/real.py"), &["fake_".to_string()]));
    }

    #[test]
    fn test_has_ignored_prefix() {
        assert!(has_ignored_prefix("fake_data", &["fake_".to_string()]));
        assert!(has_ignored_prefix("mock_dir", &["mock_".to_string()]));
        assert!(!has_ignored_prefix("real_data", &["fake_".to_string()]));
    }

    #[test]
    fn test_find_files_by_extension() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.py"), "").unwrap();
        fs::write(tmp.path().join("b.rs"), "").unwrap();
        fs::write(tmp.path().join("c.txt"), "").unwrap();
        assert_eq!(find_files_by_extension(tmp.path(), "py").len(), 1);
        assert_eq!(find_files_by_extension(tmp.path(), "rs").len(), 1);
        assert_eq!(find_files_by_extension(tmp.path(), "txt").len(), 1);
    }

    #[test]
    fn test_find_source_files_with_ignore() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.py"), "").unwrap();
        let fake_dir = tmp.path().join("fake_data");
        fs::create_dir(&fake_dir).unwrap();
        fs::write(fake_dir.join("b.py"), "").unwrap();

        assert_eq!(find_source_files(tmp.path()).len(), 2);

        let ignore = vec!["fake_".to_string()];
        assert_eq!(find_source_files_with_ignore(tmp.path(), &ignore).len(), 1);
    }
}
