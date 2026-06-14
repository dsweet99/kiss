//! Regression test for repository-level fixture boundaries.
//!
//! `tests/fake_python` and `tests/fake_rust` contain intentionally pathological
//! files used by focused unit tests. Default `kiss check` should not treat those
//! fixture corpora as project code.

use std::fs;
use std::path::Path;

fn repo_kissignore_entries() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".kissignore");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn repository_kissignore_excludes_pathological_fixture_roots() {
    let entries = repo_kissignore_entries();
    for fixture_root in ["tests/fake_python/", "tests/fake_rust/"] {
        assert!(
            entries.iter().any(|entry| entry == fixture_root),
            ".kissignore must exclude {fixture_root}; entries were {entries:?}",
        );
    }
}
