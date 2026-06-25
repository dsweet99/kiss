use std::fs;
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;
use tempfile::TempDir;

use super::runners::*;
use super::rust_coverage_index::{rebuild_rust_coverage_index, rust_coverage_cache_root};

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
fn enumerate_workspace_rust_selectors_finds_cfg_test_modules() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::create_dir(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src").join("lib.rs"),
        r#"
pub fn value() -> u32 { 1 }

#[cfg(test)]
mod tests {
    #[test]
    fn gets_value() {
        assert_eq!(super::value(), 1);
    }
}
"#,
    )
    .unwrap();

    let selectors = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap();

    assert_eq!(selectors, vec!["tests::gets_value".to_string()]);
}

#[test]
fn enumerate_workspace_rust_selectors_fails_fast_on_invalid_syntax() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::create_dir(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "fn broken(\n").unwrap();

    let err = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap_err();

    assert!(err.contains("failed to parse Rust workspace file"));
    assert!(err.contains("lib.rs"));
}

#[test]
fn discover_for_paths_empty_paths_ok() {
    let tmp = TempDir::new().unwrap();
    let defs = discover_for_paths(tmp.path(), &[], None, &[]).unwrap();
    assert!(defs.is_empty());
}

#[test]
fn combined_selectors_uses_existing_rust_index_for_source_changes() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    let lib = src.join("lib.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();
    write_rust_cov_entry(
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

    let plan = combined_selectors(tmp.path(), std::slice::from_ref(&lib), &[], None, &[]).unwrap();

    assert_eq!(plan.rust_selectors, vec!["tests::gets_value".to_string()]);
    assert_eq!(plan.rust_source_paths, vec![lib]);
    assert!(plan.rust_source_population_paths.is_empty());
}

#[test]
fn combined_selectors_marks_missing_rust_index_for_population() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    let lib = src.join("lib.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();

    let plan = combined_selectors(tmp.path(), std::slice::from_ref(&lib), &[], None, &[]).unwrap();

    assert!(plan.rust_selectors.is_empty());
    assert_eq!(plan.rust_source_paths, vec![lib.clone()]);
    assert_eq!(plan.rust_source_population_paths, vec![lib]);
}

fn write_rust_cov_entry(
    repo_root: &Path,
    name: &str,
    selector: &str,
    status: TestStatus,
    coverage: RustLineCoverage,
) {
    let path = rust_coverage_cache_root(repo_root)
        .join("entries")
        .join(format!("{name}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let entry = serde_json::json!({
        "schema_version": "rust-llvm-cov-cache-v1",
        "selector": selector,
        "status": status,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": coverage,
    });
    fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}

#[test]
fn combined_selectors_empty_without_sources() {
    let tmp = TempDir::new().unwrap();
    let plan = combined_selectors(tmp.path(), &[], &[], None, &[]).unwrap();
    assert!(plan.py_selectors.is_empty());
    assert!(plan.rust_selectors.is_empty());
    assert!(plan.rust_source_paths.is_empty());
    assert!(plan.rust_source_population_paths.is_empty());
}

#[test]
fn build_pytest_argv_non_empty() {
    let py = build_pytest_argv(&["a.py::t".into()], &["-q".into()]);
    assert_eq!(py[0], "python");
    assert!(py.iter().any(|s| s == "pytest"));
}

#[test]
fn build_cargo_llvm_cov_dry_run_argv_places_selector_before_extra() {
    let argv =
        build_cargo_llvm_cov_dry_run_argv("smoke_sub", &["--exact".into(), "--nocapture".into()]);

    assert_eq!(
        argv[0..5],
        ["cargo", "llvm-cov", "test", "--json", "--output-path"]
    );
    assert_eq!(argv[5], "<coverage.json>");
    assert_eq!(argv[6], "smoke_sub");
    assert_eq!(argv[7], "--");
    assert_eq!(argv[8], "--exact");
    assert_eq!(argv[9], "--nocapture");
}

#[test]
fn rslip_request_from_parts_uses_selector_and_kiss_cache() {
    let tmp = TempDir::new().unwrap();
    let req = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &["-q".to_string()],
        "3.12.1",
        "8.2.0",
        true,
    )
    .unwrap();

    assert_eq!(req.nodeid, "tests/test_app.py::test_ok");
    assert_eq!(req.cwd, tmp.path());
    assert_eq!(req.source_root, tmp.path());
    assert_eq!(req.pytest_args, vec!["-q"]);
    assert_eq!(req.python_version, "3.12.1");
    assert_eq!(req.pytest_version, "8.2.0");
    assert_eq!(req.cache_root, tmp.path().join(".kiss").join("rslip_cache"));
    assert!(req.force_rerun);
}

#[test]
fn rslip_request_from_parts_rejects_python_before_312() {
    let tmp = TempDir::new().unwrap();
    let err = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &[],
        "3.11.9",
        "8.2.0",
        false,
    )
    .unwrap_err();

    assert!(err.contains("Python 3.12+"));
}

#[test]
fn rslip_request_from_parts_accepts_python_after_312() {
    let tmp = TempDir::new().unwrap();
    let req = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &[],
        "3.13.0",
        "8.2.0",
        false,
    )
    .unwrap();

    assert_eq!(req.python_version, "3.13.0");
}

#[test]
fn rust_llvm_cov_request_from_parts_uses_selector_extra_and_kiss_cache() {
    let tmp = TempDir::new().unwrap();
    let req = rust_llvm_cov_request_from_parts(
        tmp.path(),
        "smoke_sub",
        &["--exact".to_string()],
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        true,
    )
    .unwrap();

    assert_eq!(req.selector, "smoke_sub");
    assert_eq!(req.cwd, tmp.path());
    assert_eq!(req.source_root, tmp.path());
    assert_eq!(req.cargo_args, Vec::<String>::new());
    assert_eq!(req.test_args, vec!["--exact"]);
    assert_eq!(req.llvm_cov_version, "cargo-llvm-cov 0.6.0");
    assert_eq!(req.rustc_version, "rustc 1.88.0");
    assert_eq!(
        req.cache_root,
        tmp.path().join(".kiss").join("rust_llvm_cov_cache")
    );
    assert!(req.force_rerun);
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
    fs::write(&lib, "def f(): pass\n").unwrap();
    fs::write(&tst, "def test_f(): pass\n").unwrap();
    let paths = vec![lib.clone(), tst.clone()];
    let (src, tst_paths) = partition_changed_paths(&paths);
    assert!(src.iter().any(|p| p == &lib));
    assert!(tst_paths.iter().any(|p| p == &tst));
}

#[test]
fn collect_selectors_from_defs_smoke() {
    use std::path::PathBuf;
    let defs: Vec<crate::test_discovery::DefEntry> = vec![(
        PathBuf::from("/x/a.py"),
        "f".into(),
        1,
        Some(vec![(PathBuf::from("/x/test_a.py"), "test_f".into())]),
    )];
    let s = collect_selectors_from_defs(&defs);
    assert!(
        s.iter()
            .any(|(p, id)| p.ends_with("test_a.py") && id == "test_f")
    );
}
