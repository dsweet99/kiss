use ignore::WalkState;
use std::fs;
use tempfile::TempDir;

use super::*;

#[test]
fn test_language_from_path() {
    assert_eq!(
        Language::from_path(std::path::Path::new("foo.py")),
        Some(Language::Python)
    );
    assert_eq!(
        Language::from_path(std::path::Path::new("bar.rs")),
        Some(Language::Rust)
    );
    assert_eq!(
        Language::from_path(std::path::Path::new("Foo.PY")),
        Some(Language::Python)
    );
    assert_eq!(
        Language::from_path(std::path::Path::new("Bar.RS")),
        Some(Language::Rust)
    );
    assert_eq!(Language::from_path(std::path::Path::new("file.txt")), None);
}

#[test]
fn test_language_extension() {
    assert_eq!(Language::Python.extension(), "py");
    assert_eq!(Language::Rust.extension(), "rs");
}

#[test]
fn test_source_file_struct() {
    let sf = SourceFile {
        path: std::path::PathBuf::from("test.py"),
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
fn test_find_python_files_uppercase_extension() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("mod.PY"), "").unwrap();
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
        std::path::Path::new("tests/fake_python/foo.py"),
        &["fake_".to_string()]
    ));
    assert!(should_ignore(
        std::path::Path::new("mock_data/test.rs"),
        &["mock_".to_string()]
    ));
    assert!(!should_ignore(
        std::path::Path::new("src/main.rs"),
        &["fake_".to_string()]
    ));
    assert!(!should_ignore(
        std::path::Path::new("tests/real.py"),
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

// === Bug-hunting tests ===

#[test]
fn test_should_ignore_does_not_match_filenames() {
    // Ignore prefixes should match directory components, not filenames.
    // A file named "test_utils.py" under "src/" should NOT be ignored
    // just because its name starts with "test_".
    assert!(
        !should_ignore(
            std::path::Path::new("src/test_utils.py"),
            &["test_".to_string()]
        ),
        "should_ignore should not match filename prefixes, only directory components"
    );
}

#[test]
fn test_always_ignored_includes_env_dir() {
    // Many Python projects use "env/" for virtualenvs, not just ".venv" or "venv".
    assert!(
        is_always_ignored("env"),
        "'env' should be always ignored (common virtualenv directory)"
    );
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
