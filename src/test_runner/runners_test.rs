use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::runners::*;

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
fn discover_for_paths_empty_paths_ok() {
    let tmp = TempDir::new().unwrap();
    let defs = discover_for_paths(tmp.path(), &[], None, &[]).unwrap();
    assert!(defs.is_empty());
}

#[test]
fn combined_selectors_empty_without_sources() {
    let tmp = TempDir::new().unwrap();
    let (py, rs) = combined_selectors(tmp.path(), &[], &[], None, &[]).unwrap();
    assert!(py.is_empty());
    assert!(rs.is_empty());
}

#[test]
fn build_pytest_and_cargo_argv_non_empty() {
    let py = build_pytest_argv(&["a.py::t".into()], &["-q".into()]);
    assert_eq!(py[0], "python");
    assert!(py.iter().any(|s| s == "pytest"));
    let c = build_cargo_test_argv(&["smoke_sub".into()], &[]);
    assert_eq!(c[0..4], ["cargo", "test", "--", "smoke_sub"]);
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
fn shlex_quote_spaces() {
    assert!(shlex_quote("a b").contains('\''));
}

#[test]
#[cfg(unix)]
fn run_command_true_zero() {
    let tmp = TempDir::new().unwrap();
    let code =
        run_command_inherit(&["sh".into(), "-c".into(), "exit 0".into()], tmp.path()).unwrap();
    assert_eq!(code, 0);
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
