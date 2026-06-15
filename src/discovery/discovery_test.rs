use ignore::WalkState;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use super::*;

const FIXTURE_BOUNDARY_KISSIGNORE: &str = "tests/fake_python/\ntests/fake_rust/\ntests/fixtures/\n";

const PATHOLOGICAL_FIXTURE_FILES: &[&str] = &[
    "tests/fake_python/pathological.py",
    "tests/fake_rust/pathological.rs",
    "tests/fixtures/pathological.py",
];

struct CwdGuard(PathBuf);

impl CwdGuard {
    fn enter(path: &std::path::Path) -> Self {
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self(old)
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

fn create_fixture_file(root: &Path, rel_path: &str) {
    let path = root.join(rel_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

fn write_fixture_boundary_kissignore(root: &Path) {
    fs::write(root.join(".kissignore"), FIXTURE_BOUNDARY_KISSIGNORE).unwrap();
}

fn create_pathological_fixture_files(root: &Path) {
    for rel_path in PATHOLOGICAL_FIXTURE_FILES {
        create_fixture_file(root, rel_path);
    }
}

fn discovered_relative_paths(root: &Path) -> Vec<PathBuf> {
    find_source_files(root)
        .iter()
        .map(|file| file.path.strip_prefix(root).unwrap().to_path_buf())
        .collect()
}

fn assert_has_path(paths: &[PathBuf], rel_path: &str) {
    assert!(
        paths.contains(&PathBuf::from(rel_path)),
        "expected discovered paths to contain {rel_path}; paths were {paths:?}",
    );
}

fn assert_lacks_path(paths: &[PathBuf], rel_path: &str) {
    assert!(
        !paths.contains(&PathBuf::from(rel_path)),
        "expected discovered paths to exclude {rel_path}; paths were {paths:?}",
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
fn kissignore_excludes_pathological_test_fixtures() {
    let tmp = TempDir::new().unwrap();
    create_fixture_file(tmp.path(), "src/lib.py");
    create_fixture_file(tmp.path(), "src/lib.rs");
    create_fixture_file(tmp.path(), "tests/unit/test_real.py");
    create_pathological_fixture_files(tmp.path());
    write_fixture_boundary_kissignore(tmp.path());

    let paths = discovered_relative_paths(tmp.path());

    assert_has_path(&paths, "src/lib.py");
    assert_has_path(&paths, "src/lib.rs");
    assert_has_path(&paths, "tests/unit/test_real.py");
    for rel_path in PATHOLOGICAL_FIXTURE_FILES {
        assert_lacks_path(&paths, rel_path);
    }
}

#[test]
fn repo_root_discovery_uses_kissignore_for_fixture_boundaries() {
    let tmp = TempDir::new().unwrap();
    let fake_py = tmp.path().join("tests/fake_python");
    let fixtures = tmp.path().join("tests/fixtures");
    create_fixture_file(tmp.path(), "src/lib.py");
    create_fixture_file(tmp.path(), "fake_app/real.py");
    create_pathological_fixture_files(tmp.path());
    write_fixture_boundary_kissignore(tmp.path());

    let paths = discovered_relative_paths(tmp.path());

    assert_has_path(&paths, "src/lib.py");
    assert_has_path(&paths, "fake_app/real.py");
    for rel_path in PATHOLOGICAL_FIXTURE_FILES {
        assert_lacks_path(&paths, rel_path);
    }

    let fixture_files = find_source_files(&fake_py);
    assert!(
        fixture_files
            .iter()
            .any(|file| file.path.ends_with("pathological.py")),
        "explicit fixture-root discovery should still work"
    );
    let nested_fixture_files = find_source_files(&fixtures);
    assert!(
        nested_fixture_files
            .iter()
            .any(|file| file.path.ends_with("pathological.py")),
        "explicit tests/fixtures discovery should still work"
    );
}

#[test]
fn relative_repo_root_discovery_uses_kissignore_for_fixture_boundaries() {
    let tmp = TempDir::new().unwrap();
    create_fixture_file(tmp.path(), "src/lib.py");
    create_pathological_fixture_files(tmp.path());
    write_fixture_boundary_kissignore(tmp.path());

    let _cwd = CwdGuard::enter(tmp.path());
    let files = find_source_files(std::path::Path::new("."));
    let paths = files
        .iter()
        .map(|file| {
            file.path
                .canonicalize()
                .unwrap()
                .strip_prefix(tmp.path())
                .unwrap()
                .to_path_buf()
        })
        .collect::<Vec<_>>();

    assert_has_path(&paths, "src/lib.py");
    for rel_path in PATHOLOGICAL_FIXTURE_FILES {
        assert_lacks_path(&paths, rel_path);
    }
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
fn test_should_ignore_matches_filenames() {
    // Per CLI help: --ignore=PREFIX ignores files/directories starting with PREFIX.
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
