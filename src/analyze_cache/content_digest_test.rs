use std::fs;

use tempfile::TempDir;

use super::content_digest::{content_digests_for_paths, verify_content_digests};

#[test]
fn content_digest_is_directly_callable_from_sibling_test_module() {
    let digest = super::content_digest::content_digest(b"abc");
    let changed = super::content_digest::content_digest(b"abd");
    let empty = super::content_digest::content_digest(b"");

    assert_eq!(empty, 0);
    assert_ne!(digest, changed);
}

#[test]
fn content_digest_verification_rejects_changed_or_missing_files() {
    let tmp = TempDir::new().unwrap();
    let a = tmp.path().join("a.py");
    let b = tmp.path().join("b.py");
    fs::write(&a, "def a():\n    return 1\n").unwrap();
    fs::write(&b, "def b():\n    return 2\n").unwrap();

    let stored = content_digests_for_paths(&[b.clone(), a.clone()]);
    assert_eq!(stored[0].0, a.to_string_lossy());
    assert!(verify_content_digests(
        &stored,
        std::slice::from_ref(&a),
        std::slice::from_ref(&b)
    ));

    fs::write(&a, "def c():\n    return 3\n").unwrap();
    if verify_content_digests(&stored, std::slice::from_ref(&a), std::slice::from_ref(&b)) {
        panic!("same-size content changes must invalidate the cache");
    }
    fs::remove_file(&b).unwrap();
    assert!(!verify_content_digests(
        &stored,
        std::slice::from_ref(&a),
        std::slice::from_ref(&b)
    ));
}

#[test]
fn content_digest_verification_rejects_missing_and_extra_entries() {
    let tmp = TempDir::new().unwrap();
    let py_path = tmp.path().join("f.py");
    let rs_path = tmp.path().join("f.rs");
    fs::write(&py_path, "def foo():\n    return 1\n").unwrap();
    fs::write(&rs_path, "fn foo() -> i32 { 1 }\n").unwrap();

    let stored = content_digests_for_paths(&[py_path.clone(), rs_path.clone()]);
    assert!(verify_content_digests(
        &stored,
        std::slice::from_ref(&py_path),
        std::slice::from_ref(&rs_path)
    ));

    let missing_rs = content_digests_for_paths(std::slice::from_ref(&py_path));
    assert!(
        !verify_content_digests(
            &missing_rs,
            std::slice::from_ref(&py_path),
            std::slice::from_ref(&rs_path)
        ),
        "stored digests must include both Python and Rust files"
    );

    let extra_path = tmp.path().join("extra.py");
    fs::write(&extra_path, "def extra():\n    return 2\n").unwrap();
    let with_extra = content_digests_for_paths(&[py_path.clone(), rs_path, extra_path]);
    assert!(
        !verify_content_digests(&with_extra, std::slice::from_ref(&py_path), &[]),
        "extra stored digests must not verify a smaller file universe"
    );
}
