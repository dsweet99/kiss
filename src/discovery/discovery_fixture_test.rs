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
    fn enter(path: &Path) -> Self {
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
    let files = find_source_files(Path::new("."));
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
