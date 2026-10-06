use std::fs;

use tempfile::tempdir;

use super::identity::current_python_execution_identity;
use crate::test_runner::runners::detect_rslip_versions;

#[test]
fn execution_identity_is_memoized_within_cycle() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }
    let args: Vec<String> = vec![];
    super::identity_memo::clear_python_execution_identity_memo();
    let first = current_python_execution_identity(repo, &args).unwrap();

    fs::write(repo.join("app.py"), b"x = 2\n").unwrap();
    let second = current_python_execution_identity(repo, &args).unwrap();
    assert_eq!(
        first.input_fingerprint, second.input_fingerprint,
        "identity must be reused within a cycle"
    );
    super::identity_memo::clear_python_execution_identity_memo();
    let third = current_python_execution_identity(repo, &args).unwrap();
    assert_ne!(
        first.input_fingerprint, third.input_fingerprint,
        "clearing memo must recompute after source change"
    );
}
