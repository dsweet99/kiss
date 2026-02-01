use ignore::{WalkBuilder, WalkState};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    Rust,
}

impl Language {
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

    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::Rust => "rs",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub language: Language,
}

pub fn find_python_files(root: &Path) -> Vec<PathBuf> {
    find_files_by_extension(root, "py")
}
pub fn find_rust_files(root: &Path) -> Vec<PathBuf> {
    find_files_by_extension(root, "rs")
}

const ALWAYS_IGNORED: &[&str] = &["__pycache__", "node_modules", ".venv", "venv"];

fn has_ignored_prefix(name: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| name.starts_with(prefix))
}

fn is_always_ignored(name: &str) -> bool {
    ALWAYS_IGNORED.contains(&name)
}

fn should_ignore(path: &Path, ignore_prefixes: &[String]) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| has_ignored_prefix(s, ignore_prefixes) || is_always_ignored(s))
    })
}

pub fn find_source_files(root: &Path) -> Vec<SourceFile> {
    find_source_files_with_ignore(root, &[])
}

pub fn find_source_files_with_ignore(root: &Path, ignore_prefixes: &[String]) -> Vec<SourceFile> {
    let results = Mutex::new(Vec::new());
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .add_custom_ignore_filename(".kissignore")
        .build_parallel()
        .run(|| Box::new(|entry| process_source_entry(entry, ignore_prefixes, &results)));
    results.into_inner().unwrap()
}

pub fn gather_files_by_lang(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore_prefixes: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut py_files, mut rs_files) = (Vec::new(), Vec::new());
    for path in paths {
        for sf in find_source_files_with_ignore(Path::new(path), ignore_prefixes) {
            match (sf.language, lang_filter) {
                (Language::Python, None | Some(Language::Python)) => py_files.push(sf.path),
                (Language::Rust, None | Some(Language::Rust)) => rs_files.push(sf.path),
                _ => {}
            }
        }
    }
    (py_files, rs_files)
}

fn process_source_entry(
    entry: Result<ignore::DirEntry, ignore::Error>,
    ignore_prefixes: &[String],
    results: &Mutex<Vec<SourceFile>>,
) -> WalkState {
    let Ok(entry) = entry else {
        return WalkState::Continue;
    };
    if !entry.file_type().is_some_and(|ft| ft.is_file()) {
        return WalkState::Continue;
    }
    let path = entry.into_path();
    if should_ignore(&path, ignore_prefixes) {
        return WalkState::Continue;
    }
    if let Some(language) = Language::from_path(&path) {
        results.lock().unwrap().push(SourceFile { path, language });
    }
    WalkState::Continue
}

fn find_files_by_extension(root: &Path, ext: &str) -> Vec<PathBuf> {
    let results = Mutex::new(Vec::new());
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .add_custom_ignore_filename(".kissignore")
        .build_parallel()
        .run(|| Box::new(|entry| process_ext_entry(entry, ext, &results)));
    results.into_inner().unwrap()
}

fn process_ext_entry(
    entry: Result<ignore::DirEntry, ignore::Error>,
    ext: &str,
    results: &Mutex<Vec<PathBuf>>,
) -> WalkState {
    let Ok(entry) = entry else {
        return WalkState::Continue;
    };
    if !entry.file_type().is_some_and(|ft| ft.is_file()) {
        return WalkState::Continue;
    }
    if entry.path().extension().is_some_and(|e| e == ext) {
        results.lock().unwrap().push(entry.into_path());
    }
    WalkState::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_language_from_path() {
        assert_eq!(
            Language::from_path(Path::new("foo.py")),
            Some(Language::Python)
        );
        assert_eq!(
            Language::from_path(Path::new("bar.rs")),
            Some(Language::Rust)
        );
        assert_eq!(Language::from_path(Path::new("file.txt")), None);
    }

    #[test]
    fn test_language_extension() {
        assert_eq!(Language::Python.extension(), "py");
        assert_eq!(Language::Rust.extension(), "rs");
    }

    #[test]
    fn test_source_file_struct() {
        let sf = SourceFile {
            path: PathBuf::from("test.py"),
            language: Language::Python,
        };
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
        assert!(should_ignore(
            Path::new("tests/fake_python/foo.py"),
            &["fake_".to_string()]
        ));
        assert!(should_ignore(
            Path::new("mock_data/test.rs"),
            &["mock_".to_string()]
        ));
        assert!(!should_ignore(
            Path::new("src/main.rs"),
            &["fake_".to_string()]
        ));
        assert!(!should_ignore(
            Path::new("tests/real.py"),
            &["fake_".to_string()]
        ));
        assert!(
            is_always_ignored("node_modules")
                && is_always_ignored("__pycache__")
                && !is_always_ignored("src")
        );
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

    #[test]
    fn test_gather_files_by_lang_empty_input() {
        let (py, rs) = gather_files_by_lang(&[], None, &[]);
        assert!(py.is_empty());
        assert!(rs.is_empty());
    }

    #[test]
    fn test_kissignore_file() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.py"), "").unwrap();
        let ignored_dir = tmp.path().join("ignored");
        fs::create_dir(&ignored_dir).unwrap();
        fs::write(ignored_dir.join("b.py"), "").unwrap();
        fs::write(tmp.path().join(".kissignore"), "ignored/\n").unwrap();

        let files = find_source_files_with_ignore(tmp.path(), &[]);
        assert_eq!(files.len(), 1);
        assert!(files[0].path.ends_with("a.py"));
    }

    #[test]
    fn test_process_source_entry_and_ext_entry() {
        use std::sync::Mutex;
        // Test process_source_entry with a valid file
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("test.py"), "").unwrap();
        let results = Mutex::new(Vec::new());
        for entry in ignore::WalkBuilder::new(tmp.path()).build() {
            let state = process_source_entry(entry, &[], &results);
            assert!(matches!(state, WalkState::Continue));
        }
        assert!(!results.into_inner().unwrap().is_empty());

        // Test process_ext_entry
        let results2 = Mutex::new(Vec::new());
        for entry in ignore::WalkBuilder::new(tmp.path()).build() {
            let state = process_ext_entry(entry, "py", &results2);
            assert!(matches!(state, WalkState::Continue));
        }
        assert!(!results2.into_inner().unwrap().is_empty());
    }
}
