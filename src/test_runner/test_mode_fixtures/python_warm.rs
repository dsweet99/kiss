use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use kiss::rpytest_runner::TestStatus;
use tempfile::TempDir;

use super::git::{commit_all, ensure_main_branch, init_git};

pub(crate) const PY_SELECTOR: &str = "tests/test_app.py::test_value";

fn write_python_tree(root: &Path, value: i32) -> PathBuf {
    fs::create_dir_all(root.join("pkg")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("pkg").join("__init__.py"), "").unwrap();
    let app = root.join("pkg").join("app.py");
    fs::write(&app, format!("def value():\n    return {value}\n")).unwrap();
    fs::write(
        root.join("tests").join("test_app.py"),
        "from pkg.app import value\n\ndef test_value():\n    assert value() == value()\n",
    )
    .unwrap();
    fs::write(
        root.join("pytest.ini"),
        "[pytest]\ntestpaths = tests\npython_files = test_*.py\n",
    )
    .unwrap();
    app
}

pub(crate) fn publish_python_record(root: &Path) {
    crate::test_runner::lang_python::records::store_records(
        root,
        &[(PY_SELECTOR, TestStatus::Passed)],
    );
}

pub(crate) fn warm_python_demo(tmp: &TempDir) -> PathBuf {
    init_git(tmp);
    ensure_main_branch(tmp.path());
    let app = write_python_tree(tmp.path(), 1);
    publish_python_record(tmp.path());
    commit_all(tmp.path(), "warm-python");
    app
}

fn persistent_warm_python_repo() -> PathBuf {
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-warm-python-fixture");
        if root.join("pkg/app.py").is_file() && root.join(".kiss").is_dir() {
            // Ensure cheap max_num_tests count works under threshold-0 sibling gates.
            crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
                &root,
                &[],
                &[PY_SELECTOR.to_string()],
                &[],
            );
            return root;
        }
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tmp = TempDir::new().unwrap();
        let _ = warm_python_demo(&tmp);
        let status = std::process::Command::new("cp")
            .args([
                "-a",
                &format!("{}/.", tmp.path().display()),
                &format!("{}/", root.display()),
            ])
            .status()
            .expect("cp warm python fixture");
        assert!(status.success());
        crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
            &root,
            &[],
            &[PY_SELECTOR.to_string()],
            &[],
        );
        root
    })
    .clone()
}

fn warm_python_repo_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Use the persistent warm python fixture in-place (no rebuild/publish).
pub(crate) fn with_locked_warm_python_repo<T>(f: impl FnOnce(&Path, PathBuf) -> T) -> T {
    let _lock = warm_python_repo_lock();
    let repo = persistent_warm_python_repo();
    let app = repo.join("pkg").join("app.py");
    f(&repo, app)
}

pub(crate) fn edit_python_source(app: &Path, value: i32) {
    fs::write(app, format!("def value():\n    return {value}\n")).unwrap();
}

pub(crate) fn refresh_python_selectors_after_edit(root: &Path) {
    assert!(
        crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
            root,
            &[],
            &[PY_SELECTOR.to_string()],
            &[],
        )
    );
}
