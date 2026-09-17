use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use kiss::rslip::LineCoverage;
use tempfile::TempDir;

use crate::test_runner::python_coverage_index::{
    python_coverage_cache_root, rebuild_python_coverage_index,
    write_python_population_manifest_for_args,
};

use super::git::{commit_all, ensure_main_branch, init_git};

pub(crate) const PY_COVERING_SELECTOR: &str = "tests/test_app.py::test_value";

fn write_python_entry(repo_root: &Path, name: &str, selector: &str, coverage: LineCoverage) {
    let path = python_coverage_cache_root(repo_root)
        .unwrap()
        .join("entries")
        .join(format!("{name}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let entry = serde_json::json!({
        "schema_version": kiss::rslip::CACHE_SCHEMA_VERSION,
        "nodeid": selector,
        "status": TestStatus::Passed,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": coverage,
    });
    fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}

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

pub(crate) fn publish_python_covering(root: &Path, app: &Path) {
    write_python_entry(
        root,
        "py",
        PY_COVERING_SELECTOR,
        LineCoverage {
            files: BTreeMap::from([(
                app.to_string_lossy().to_string(),
                std::collections::BTreeSet::from([1, 2]),
            )]),
        },
    );
    // rebuild publishes the population from entry selectors; no second publish.
    rebuild_python_coverage_index(root).unwrap();
}

pub(crate) fn warm_python_covering_demo(tmp: &TempDir) -> PathBuf {
    init_git(tmp);
    ensure_main_branch(tmp.path());
    let app = write_python_tree(tmp.path(), 1);
    publish_python_covering(tmp.path(), &app);
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
                &[PY_COVERING_SELECTOR.to_string()],
                &[],
            );
            return root;
        }
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tmp = TempDir::new().unwrap();
        let _ = warm_python_covering_demo(&tmp);
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
            &[PY_COVERING_SELECTOR.to_string()],
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

/// Use the persistent warm python covering fixture in-place (no rebuild/publish).
pub(crate) fn with_locked_warm_python_repo<T>(f: impl FnOnce(&Path, PathBuf) -> T) -> T {
    let _lock = warm_python_repo_lock();
    let repo = persistent_warm_python_repo();
    let app = repo.join("pkg").join("app.py");
    f(&repo, app)
}

pub(crate) fn edit_python_covered_source(app: &Path, value: i32) {
    fs::write(app, format!("def value():\n    return {value}\n")).unwrap();
}

pub(crate) fn rewrite_python_population_after_edit(root: &Path) {
    write_python_population_manifest_for_args(root, &[PY_COVERING_SELECTOR.to_string()], &[])
        .unwrap();
}
