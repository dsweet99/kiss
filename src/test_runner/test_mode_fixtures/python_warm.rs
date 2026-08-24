use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rpytest_runner::TestStatus;
use rslip::LineCoverage;
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
        "schema_version": rslip::CACHE_SCHEMA_VERSION,
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
                std::collections::BTreeSet::from([2]),
            )]),
        },
    );
    rebuild_python_coverage_index(root).unwrap();
    write_python_population_manifest_for_args(root, &[PY_COVERING_SELECTOR.to_string()], &[])
        .unwrap();
}

pub(crate) fn warm_python_covering_demo(tmp: &TempDir) -> PathBuf {
    init_git(tmp);
    ensure_main_branch(tmp.path());
    let app = write_python_tree(tmp.path(), 1);
    publish_python_covering(tmp.path(), &app);
    commit_all(tmp.path(), "warm-python");
    app
}

pub(crate) fn edit_python_covered_source(app: &Path, value: i32) {
    fs::write(app, format!("def value():\n    return {value}\n")).unwrap();
}

pub(crate) fn rewrite_python_population_after_edit(root: &Path) {
    write_python_population_manifest_for_args(root, &[PY_COVERING_SELECTOR.to_string()], &[])
        .unwrap();
}
