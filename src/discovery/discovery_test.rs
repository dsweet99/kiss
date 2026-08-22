use ignore::WalkState;
use std::fs;
use tempfile::TempDir;

use super::*;

#[test]
fn merge_check_ignore_prefixes_always_includes_defaults() {
    assert_eq!(
        merge_check_ignore_prefixes(&["custom/".to_string()]),
        vec!["fake_", "fixtures", "custom"]
    );
}

#[test]
fn test_normalize_ignore_prefixes_trims_and_drops_empty() {
    let out = normalize_ignore_prefixes(&[
        "src/".to_string(),
        "src".to_string(),
        String::new(),
        "  ".to_string(),
    ]);
    assert_eq!(out, vec!["src", "src"]);
}

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
    assert_eq!(
        Language::from_path(std::path::Path::new("frag.inc")),
        Some(Language::Rust)
    );
    assert!(Language::is_rust_path(std::path::Path::new("x.INC")));
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
    assert!(ignore_prefix_matches("fake_data", "fake_"));
    assert!(ignore_prefix_matches("mock_dir", "mock_"));
    assert!(!ignore_prefix_matches("real_data", "fake_"));
}

#[test]
fn ignore_prefix_matches_component_and_path_forms() {
    assert!(
        ignore_prefix_matches("tests/fake_python/test_x.py", "fake_"),
        "component form: fake_ must match directory fake_python"
    );
    assert!(
        ignore_prefix_matches("src/fake_utils.py", "fake_"),
        "component form: fake_ must match filename fake_utils.py"
    );
    assert!(
        ignore_prefix_matches("tests/slow/test_b.py", "tests/slow"),
        "path form: tests/slow must match descendants"
    );
    assert!(ignore_prefix_matches("sub/x.py", "sub"));
    assert!(!ignore_prefix_matches("tests/test_app.py", "fake_"));
    assert!(!ignore_prefix_matches("tests/fast/test_a.py", "tests/slow"));
    assert!(
        ignore_prefix_matches("/tmp/repo/lib/slow/mod.py", "lib/slow"),
        "path form must match consecutive components on an absolute walk path"
    );
    assert!(!ignore_prefix_matches(
        "/tmp/repo/lib/fast/mod.py",
        "lib/slow"
    ));
    assert!(path_ignored_by_prefixes(
        "tests/fake_python/test_x.py::test_x",
        &["fake_".to_string(), "fixtures".to_string()]
    ));
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
fn test_should_ignore_matches_filenames() {
    assert!(
        should_ignore(
            std::path::Path::new("src/test_utils.py"),
            &["test_".to_string()]
        ),
        "should_ignore should match filename prefixes per documented --ignore behavior"
    );
    assert!(
        should_ignore(std::path::Path::new("big.py"), &["big".to_string()]),
        "root-level files should be ignored when filename starts with PREFIX"
    );
    assert!(
        should_ignore(
            std::path::Path::new("tests/slow/test_b.py"),
            &["tests/slow".to_string()]
        ),
        "path-form PREFIX must ignore descendants in check the same way test does"
    );
    assert!(
        should_ignore(
            std::path::Path::new("/tmp/repo/lib/slow/mod.py"),
            &["lib/slow".to_string()]
        ),
        "path-form PREFIX must ignore absolute WalkBuilder paths"
    );
}

#[test]
fn find_source_files_path_form_ignore_on_absolute_root() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.py"), "").unwrap();
    let slow = tmp.path().join("lib/slow");
    fs::create_dir_all(&slow).unwrap();
    fs::write(slow.join("mod.py"), "").unwrap();
    let files = find_source_files_with_ignore(tmp.path(), &["lib/slow".to_string()]);
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].path.ends_with("a.py"));
}

#[test]
fn test_always_ignored_includes_env_dir() {
    assert!(
        is_always_ignored("env"),
        "'env' should be always ignored (common virtualenv directory)"
    );
}

#[test]
fn test_process_source_entry_and_ext_entry() {
    use std::sync::Mutex;

    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("test.py"), "").unwrap();
    let results = Mutex::new(Vec::new());
    for entry in ignore::WalkBuilder::new(tmp.path()).build() {
        let state = process_source_entry(entry, &[], &results);
        assert!(matches!(state, WalkState::Continue));
    }
    assert!(!results.into_inner().unwrap().is_empty());

    let results2 = Mutex::new(Vec::new());
    for entry in ignore::WalkBuilder::new(tmp.path()).build() {
        let state = process_ext_entry(entry, "py", &results2);
        assert!(matches!(state, WalkState::Continue));
    }
    assert!(!results2.into_inner().unwrap().is_empty());
}
