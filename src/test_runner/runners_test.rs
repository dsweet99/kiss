use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;
use tempfile::TempDir;

use super::runners::*;
use super::rust_coverage_index::{
    rebuild_rust_coverage_index, write_rust_population_manifest_for_args, write_test_entry,
};

#[test]
fn py_selector_uses_double_colon() {
    let p = Path::new("/tmp/t.py");
    assert_eq!(py_selector(p, "test_foo"), "/tmp/t.py::test_foo");
}

#[test]
fn py_selector_class_method() {
    let p = Path::new("/w/test_m.py");
    assert_eq!(py_selector(p, "C::test_m"), "/w/test_m.py::C::test_m");
}

#[test]
fn shell_quote_simple() {
    let v = vec![
        "python".into(),
        "-m".into(),
        "pytest".into(),
        "a.py::t".into(),
    ];
    let s = shell_quote_line(&v);
    assert!(s.contains("python"));
    assert!(s.contains("pytest"));
}

#[test]
fn merge_exit_codes_max() {
    assert_eq!(merge_exit_codes(0, 3), 3);
    assert_eq!(merge_exit_codes(2, 1), 2);
}

#[test]
fn enumerate_tests_in_changed_files_finds_py() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("test_z.py"),
        "def test_one():\n    assert 1\n",
    )
    .unwrap();
    let paths = vec![tmp.path().join("test_z.py")];
    let got = enumerate_tests_in_changed_files(&paths).unwrap();
    assert!(got.iter().any(|(_, id)| id == "test_one"));
}

#[test]
fn enumerate_tests_in_changed_files_errors_on_bad_rs() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("broken.rs"), "fn broken(\n").unwrap();
    let paths = vec![tmp.path().join("broken.rs")];
    let err = enumerate_tests_in_changed_files(&paths).unwrap_err();
    assert!(err.contains("failed to parse"));
    assert!(err.contains("broken.rs"));
}

#[test]
fn combined_selectors_uses_existing_rust_index_for_source_changes() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    let lib = src.join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    write_test_entry(
        tmp.path(),
        "abc",
        "tests::gets_value",
        TestStatus::Passed,
        RustLineCoverage {
            files: std::collections::BTreeMap::from([(
                lib.to_string_lossy().to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_population_manifest_for_args(tmp.path(), &["tests::gets_value".to_string()], &[])
        .unwrap();

    let plan = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();

    assert_eq!(plan.rust_selectors, vec!["tests::gets_value".to_string()]);
    assert_eq!(plan.rust_source_paths, vec![lib]);
    assert!(!plan.rust_population_required);
}

#[test]
fn combined_selectors_repopulates_when_rust_test_args_change() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    let lib = src.join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    write_test_entry(
        tmp.path(),
        "abc",
        "tests::gets_value",
        TestStatus::Passed,
        RustLineCoverage {
            files: std::collections::BTreeMap::from([(
                lib.to_string_lossy().to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_population_manifest_for_args(tmp.path(), &["tests::gets_value".to_string()], &[])
        .unwrap();

    let plan = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &["--exact".to_string()],
        None,
        &[],
    )
    .unwrap();

    assert_eq!(plan.rust_selectors, vec!["tests::gets_value".to_string()]);
    assert!(plan.rust_population_required);
}

#[test]
fn combined_selectors_prefers_rust_changed_line_matches() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    let lib = src.join("lib.rs");
    fs::write(
        &lib,
        "pub fn first() {}\npub fn second() {}\n#[cfg(test)]\nmod tests { #[test] fn first() {} #[test] fn second() {} }\n",
    )
    .unwrap();
    write_test_entry(
        tmp.path(),
        "line1",
        "tests::first",
        TestStatus::Passed,
        RustLineCoverage {
            files: std::collections::BTreeMap::from([(
                lib.to_string_lossy().to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    write_test_entry(
        tmp.path(),
        "line2",
        "tests::second",
        TestStatus::Passed,
        RustLineCoverage {
            files: std::collections::BTreeMap::from([(
                lib.to_string_lossy().to_string(),
                std::collections::BTreeSet::from([2]),
            )]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_population_manifest_for_args(
        tmp.path(),
        &["tests::first".to_string(), "tests::second".to_string()],
        &[],
    )
    .unwrap();

    let plan = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::from([(lib.clone(), BTreeSet::from([2]))]),
        &[],
        None,
        &[],
    )
    .unwrap();

    assert_eq!(plan.rust_selectors, vec!["tests::second".to_string()]);
    assert_eq!(plan.rust_source_paths, vec![lib]);
    assert!(!plan.rust_population_required);
}

#[test]
fn combined_selectors_requires_complete_rust_population_manifest() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    let lib = src.join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    write_test_entry(
        tmp.path(),
        "abc",
        "tests::gets_value",
        TestStatus::Passed,
        RustLineCoverage {
            files: std::collections::BTreeMap::from([(
                lib.to_string_lossy().to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );

    let plan = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();

    assert_eq!(plan.rust_selectors, vec!["tests::gets_value".to_string()]);
    assert!(plan.rust_population_required);
}

#[test]
fn combined_selectors_carries_changed_rust_tests_into_population_plan() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let tests = tmp.path().join("tests");
    fs::create_dir(&src).unwrap();
    fs::create_dir(&tests).unwrap();
    let lib = src.join("lib.rs");
    let changed_test = tests.join("changed_test.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    fs::write(&changed_test, "#[test]\nfn changed_extra() {}\n").unwrap();

    let plan = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        std::slice::from_ref(&changed_test),
        &BTreeMap::new(),
        &[],
        None,
        &[changed_test
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()],
    )
    .unwrap();

    assert!(plan.rust_population_required);
    assert!(
        plan.rust_selectors
            .contains(&"tests::gets_value".to_string())
    );
    assert!(plan.rust_selectors.contains(&"changed_extra".to_string()));
}

#[test]
fn combined_selectors_marks_missing_rust_index_for_population() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    let lib = src.join("lib.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();

    let plan = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();

    assert!(plan.rust_selectors.is_empty());
    assert_eq!(plan.rust_source_paths, vec![lib]);
    assert!(plan.rust_population_required);
}

#[test]
fn combined_selectors_empty_without_sources() {
    let tmp = TempDir::new().unwrap();
    let plan = combined_selectors(tmp.path(), &[], &[], &BTreeMap::new(), &[], None, &[]).unwrap();
    assert!(plan.py_selectors.is_empty());
    assert!(plan.rust_selectors.is_empty());
    assert!(plan.rust_source_paths.is_empty());
    assert!(!plan.rust_population_required);
}

#[test]
fn build_pytest_argv_non_empty() {
    let py = build_pytest_argv(&["a.py::t".into()], &["-q".into()]);
    assert_eq!(py[0], "python");
    assert!(py.iter().any(|s| s == "pytest"));
}

#[test]
fn rust_coverage_batch_dry_run_lines_render_one_nextest_batch() {
    let selectors = vec!["alpha".to_string(), "beta".to_string()];
    let lines =
        build_rust_coverage_batch_dry_run_lines(&selectors, &["--exact".into()], 8).unwrap();

    assert_eq!(lines[0], "RUST BATCH selectors=2 jobs=8");
    assert!(lines[1].starts_with("cargo llvm-cov nextest"));
    assert!(lines[1].contains("'--build-jobs' 8"));
    assert!(lines[1].contains("'--test-threads' 8"));
    assert!(lines[1].contains("'--message-format-version' 0.1"));
    assert!(!lines[1].contains("llvm-cov test"));
    assert!(!lines[1].contains("--no-clean"));
    assert_eq!(lines[2], "RUST SELECTOR alpha");
    assert_eq!(lines[3], "RUST SELECTOR beta");
}

#[test]
fn rust_coverage_batch_dry_run_lines_return_unsupported_argument_error() {
    let selectors = vec!["alpha".to_string()];
    let err =
        build_rust_coverage_batch_dry_run_lines(&selectors, &["--format".into(), "json".into()], 8)
            .unwrap_err();

    assert!(err.contains("unsupported Rust test argument"));
    assert!(err.contains("--format"));
}

#[test]
fn shlex_quote_spaces() {
    assert!(shlex_quote("a b").contains('\''));
}

#[test]
fn partition_changed_paths_split() {
    let tmp = TempDir::new().unwrap();
    let lib = tmp.path().join("lib.py");
    let tst = tmp.path().join("test_lib.py");
    let rust_src = tmp.path().join("lib.rs");
    let rust_test = tmp.path().join("lib_test.rs");
    fs::write(&lib, "def f(): pass\n").unwrap();
    fs::write(&tst, "def test_f(): pass\n").unwrap();
    fs::write(&rust_src, "fn f() {}\n").unwrap();
    fs::write(&rust_test, "#[test]\nfn test_f() {}\n").unwrap();
    let paths = vec![
        lib.clone(),
        tst.clone(),
        rust_src.clone(),
        rust_test.clone(),
    ];
    let (src, tst_paths) = partition_changed_paths(&paths);
    assert!(src.iter().any(|p| p == &lib));
    assert!(src.iter().any(|p| p == &rust_src));
    assert!(tst_paths.iter().any(|p| p == &tst));
    assert!(tst_paths.iter().any(|p| p == &rust_test));
}
