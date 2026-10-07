use tempfile::TempDir;

use crate::test_runner::TestEnvVarGuard;
use crate::test_runner::lang_python::versions::pytest_env;

fn canonical(tmp: &TempDir) -> String {
    tmp.path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn pytest_env_tracks_repo_pythonpath() {
    let _lock = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let custom = format!("{}:src", canonical(&tmp));
    let _pythonpath = TestEnvVarGuard::set("PYTHONPATH", &custom);

    assert_eq!(pytest_env(tmp.path()).get("PYTHONPATH"), Some(&custom));
}

#[test]
fn pytest_env_ignores_foreign_pythonpath() {
    let _lock = crate::cwd_test_lock::lock();
    let _pythonpath = TestEnvVarGuard::set("PYTHONPATH", "/home/dsweet/Projects/kiss");
    let tmp = TempDir::new().unwrap();

    assert_eq!(
        pytest_env(tmp.path()).get("PYTHONPATH"),
        Some(&canonical(&tmp))
    );
}

#[test]
fn pytest_env_defaults_unset_pythonpath_to_repo_root() {
    let _lock = crate::cwd_test_lock::lock();
    unsafe { std::env::remove_var("PYTHONPATH") };
    let tmp = TempDir::new().unwrap();

    assert_eq!(
        pytest_env(tmp.path()).get("PYTHONPATH"),
        Some(&canonical(&tmp))
    );
}
