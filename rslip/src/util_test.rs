use std::path::{Path, PathBuf};

use crate::{db_path, normalize_path};

#[test]
fn util_helpers_are_directly_callable_from_tests() {
    let digest_cases = [(b"abc".as_slice(), b"abc".as_slice()), (b"abc", b"abd")];
    for (left, right) in digest_cases {
        if left == right {
            assert_eq!(
                crate::util::content_digest(left),
                crate::util::content_digest(right)
            );
        } else {
            assert_ne!(
                crate::util::content_digest(left),
                crate::util::content_digest(right)
            );
        }
    }
    let repo = Path::new("/tmp/repo");
    let db = db_path(repo);
    let rel = normalize_path(repo, Path::new("/tmp/repo/pkg/app.py"));
    let rel_from_relative = normalize_path(repo, Path::new("pkg/app.py"));

    assert_eq!(db, PathBuf::from("/tmp/repo/.kiss/rslip.json"));
    assert_eq!(rel, "pkg/app.py");
    assert_eq!(rel_from_relative, "pkg/app.py");
}

#[test]
fn util_content_digest_handles_empty_and_same_length_changes() {
    let empty_digest = crate::util::content_digest(b"");
    let first_digest = crate::util::content_digest(b"def a():\n    return 1\n");
    let second_digest = crate::util::content_digest(b"def b():\n    return 2\n");

    assert_eq!(empty_digest, "cbf29ce484222325");
    assert_ne!(
        first_digest, second_digest,
        "same-length content changes must alter the digest"
    );
}
