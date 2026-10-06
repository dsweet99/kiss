use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use super::rust_record_misses;
use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::record_digest::{covered_items_digest, test_definition_digests};
use crate::test_records::{RECORD_SCHEMA, TestRecord, records_dir, store_record};

const LIB: &str =
    "pub fn f(x: i32) -> i32 {\n    x + 1\n}\n\npub fn g(x: i32) -> i32 {\n    x * 2\n}\n";
const TESTS: &str = "#[test]\nfn t_f() {\n    assert_eq!(crate::f(1), expected());\n}\n\nfn expected() -> i32 {\n    2\n}\n";

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), LIB).unwrap();
    std::fs::write(tmp.path().join("src/tests.rs"), TESTS).unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let covered = BTreeSet::from([2]);
    let mut deps = BTreeMap::from([(
        "src/lib.rs".to_string(),
        covered_items_digest(LIB, &covered),
    )]);
    deps.extend(test_definition_digests("src/tests.rs", TESTS));
    let record = TestRecord {
        schema: RECORD_SCHEMA.to_string(),
        language: "rust".to_string(),
        test_id: "t_f".to_string(),
        identity: "id".to_string(),
        deps,
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        covered: BTreeMap::from([(
            root.join("src/lib.rs").to_string_lossy().into_owned(),
            covered,
        )]),
    };
    let dir = records_dir(tmp.path(), "rust");
    std::fs::create_dir_all(&dir).unwrap();
    store_record(&dir, &record).unwrap();
    tmp
}

fn misses(tmp: &tempfile::TempDir, identity: &str) -> Vec<String> {
    rust_record_misses(
        tmp.path(),
        identity,
        &["t_f".to_string(), "t_new".to_string()],
    )
}

#[test]
fn unchanged_sources_keep_the_record_and_unrecorded_tests_run() {
    let tmp = fixture();
    assert_eq!(misses(&tmp, "id"), vec!["t_new".to_string()]);
}

#[test]
fn edit_to_an_uncovered_function_keeps_the_record() {
    let tmp = fixture();
    std::fs::write(tmp.path().join("src/lib.rs"), LIB.replace("x * 2", "x * 3")).unwrap();
    assert_eq!(misses(&tmp, "id"), vec!["t_new".to_string()]);
}

#[test]
fn edit_to_the_covered_function_or_the_test_body_reruns_it() {
    let tmp = fixture();
    std::fs::write(tmp.path().join("src/lib.rs"), LIB.replace("x + 1", "x + 2")).unwrap();
    assert_eq!(
        misses(&tmp, "id"),
        vec!["t_f".to_string(), "t_new".to_string()]
    );
    let tmp = fixture();
    std::fs::write(
        tmp.path().join("src/tests.rs"),
        TESTS.replace("(1), ", "(0) + 1 + "),
    )
    .unwrap();
    assert_eq!(
        misses(&tmp, "id"),
        vec!["t_f".to_string(), "t_new".to_string()]
    );
}

#[test]
fn edit_to_a_helper_in_the_uncovered_test_file_reruns_its_tests() {
    let tmp = fixture();
    std::fs::write(
        tmp.path().join("src/tests.rs"),
        TESTS.replace("    2\n", "    3\n"),
    )
    .unwrap();
    assert_eq!(
        misses(&tmp, "id"),
        vec!["t_f".to_string(), "t_new".to_string()]
    );
}

#[test]
fn identity_change_reruns_every_test() {
    let tmp = fixture();
    assert_eq!(
        misses(&tmp, "other"),
        vec!["t_f".to_string(), "t_new".to_string()]
    );
}
