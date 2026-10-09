use std::fs;
use std::path::Path;

use crate::test_runner::test_mode_fixtures::{git_in, init_git};

use super::canon::canonicalize_target_request;
use super::resolve::resolve_only;
use super::resolved::{OperandClass, SourceRegion};
use super::types::{GitFocus, OperandExpr, TargetFocus, TargetRequest};

fn seed_python() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git(&tmp);
    fs::create_dir_all(tmp.path().join("pkg")).unwrap();
    fs::create_dir_all(tmp.path().join("tests")).unwrap();
    fs::write(tmp.path().join("pkg/__init__.py"), "").unwrap();
    fs::write(
        tmp.path().join("pkg/app.py"),
        "def value():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("tests/test_app.py"),
        "def test_value():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("tests/test_params.py"),
        "import pytest\n@pytest.mark.parametrize('n', [0])\ndef test_item(n):\n    assert n == 0\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "seed"])
            .status()
            .unwrap()
            .success()
    );
    tmp
}

fn req(focus: TargetFocus) -> TargetRequest {
    canonicalize_target_request(
        TargetRequest {
            focus,
            lang: None,
            ignore: Vec::new(),
        },
        None,
    )
}

fn ops(raws: &[&str]) -> TargetFocus {
    TargetFocus::Operands(
        raws.iter()
            .map(|raw| OperandExpr {
                raw: raw.to_string(),
            })
            .collect(),
    )
}

#[test]
fn workspace_uses_sentinel_region() {
    let tmp = seed_python();
    let resolved = resolve_only(tmp.path(), &req(TargetFocus::Workspace)).unwrap();
    assert_eq!(resolved.regions, vec![SourceRegion::WorkspaceAll]);
    assert!(resolved.historical_paths.is_empty());
    assert!(resolved.git_stamp.is_none());
}

#[test]
fn directory_of_only_tests_is_a_complete_slice() {
    let tmp = seed_python();
    let request = req(ops(&["tests"]));
    let resolved = resolve_only(tmp.path(), &request).unwrap();
    assert!(
        resolved.operand_classes.contains(&OperandClass::Directory),
        "{:?}",
        resolved.operand_classes
    );
    assert!(
        resolved.regions.is_empty(),
        "test files in a directory are selectors, not production regions: {:?}",
        resolved.regions
    );
    assert!(
        !resolved.direct_selectors.is_empty(),
        "directory of tests must resolve selectors: {:?}",
        resolved.direct_selectors
    );
    assert!(
        crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
            tmp.path(),
            &[],
            &resolved.direct_selectors,
            &[],
        ),
        "selector cache must record the tests this directory resolved"
    );
    let (_, complete) =
        crate::test_runner::target_request::build_slice_projection(tmp.path(), &request, &resolved);
    assert!(
        complete,
        "a directory that contains only tests must be a complete slice"
    );
}

#[test]
fn test_only_projection_has_empty_report_regions() {
    let tmp = seed_python();
    let request = req(ops(&["tests/test_app.py::test_value"]));
    let resolved = resolve_only(tmp.path(), &request).unwrap();
    let (projection, _) =
        crate::test_runner::target_request::build_slice_projection(tmp.path(), &request, &resolved);
    assert!(
        projection.report_regions().is_empty(),
        "test-only projection must not carry production regions: {:?}",
        projection.report_regions()
    );
}

#[test]
fn production_file_test_symbol_does_not_add_a_source_region() {
    let tmp = seed_python();
    fs::write(
        tmp.path().join("pkg/hidden.py"),
        "def test_hidden():\n    assert False\n",
    )
    .unwrap();
    let resolved = resolve_only(tmp.path(), &req(ops(&["pkg/hidden.py::test_hidden"]))).unwrap();
    assert!(
        resolved.regions.is_empty(),
        "a named test in a production file is that test, not the whole source: {:?}",
        resolved.regions
    );
    assert!(
        resolved
            .direct_selectors
            .iter()
            .any(|selector| selector.contains("test_hidden")),
        "{:?}",
        resolved.direct_selectors
    );
}

#[test]
fn test_only_nodeid_has_no_production_region() {
    let tmp = seed_python();
    let resolved = resolve_only(tmp.path(), &req(ops(&["tests/test_app.py::test_value"]))).unwrap();
    assert!(
        resolved.regions.is_empty(),
        "test-only target must not create a production region: {:?}",
        resolved.regions
    );
    assert!(
        resolved
            .direct_selectors
            .iter()
            .any(|selector| selector.contains("test_value")),
        "{:?}",
        resolved.direct_selectors
    );
}

#[test]
fn operand_classes_cover_file_symbol_nodeid_and_directory() {
    let tmp = seed_python();
    let root = tmp.path();
    let resolved = resolve_only(
        root,
        &req(ops(&[
            "pkg",
            "pkg/app.py",
            "pkg/app.py::value",
            "tests/test_app.py",
            "tests/test_app.py::test_value",
            "tests/test_params.py::test_item[0]",
        ])),
    )
    .unwrap();
    assert_eq!(
        resolved.operand_classes,
        vec![
            OperandClass::Directory,
            OperandClass::SourceFile,
            OperandClass::SourceSymbol,
            OperandClass::TestFile,
            OperandClass::TestSymbol,
            OperandClass::PythonNodeid,
        ]
    );
}

#[test]
fn symlink_with_a_test_filename_is_a_test_file() {
    let tmp = seed_python();
    std::os::unix::fs::symlink("pkg/app.py", tmp.path().join("test_link.py")).unwrap();
    let resolved = resolve_only(tmp.path(), &req(ops(&["test_link.py"]))).unwrap();
    assert_eq!(resolved.operand_classes, vec![OperandClass::TestFile]);
    let symbol = resolve_only(tmp.path(), &req(ops(&["test_link.py::value"]))).unwrap();
    assert_eq!(symbol.operand_classes, vec![OperandClass::TestSymbol]);
}

#[test]
fn outside_repo_and_lang_conflict_are_rejected() {
    let tmp = seed_python();
    let outside = Path::new("/tmp/kiss-target-outside.py");
    fs::write(outside, "x = 1\n").unwrap();
    let err = resolve_only(tmp.path(), &req(ops(&[outside.to_str().unwrap()]))).unwrap_err();
    assert!(
        err.contains("escapes repository root")
            || err.contains("not found")
            || err.contains("cannot")
    );
    let mut filtered = req(ops(&["pkg/app.py"]));
    filtered.lang = Some(super::types::LangFilter::Rust);
    let err = resolve_only(tmp.path(), &filtered).unwrap_err();
    assert!(err.contains("--lang") || err.contains("rust"));
}

#[test]
fn commit_deleted_path_is_historical_not_a_region() {
    let tmp = seed_python();
    let root = tmp.path();
    fs::remove_file(root.join("pkg/app.py")).unwrap();
    let resolved = resolve_only(root, &req(TargetFocus::Git(GitFocus::Commit))).unwrap();
    assert!(
        resolved
            .historical_paths
            .iter()
            .any(|path| path.ends_with("pkg/app.py")),
        "{:?}",
        resolved.historical_paths
    );
    assert!(!resolved.regions.iter().any(|region| match region {
        SourceRegion::FileAll { path } | SourceRegion::FileLines { path, .. } => {
            path.ends_with("pkg/app.py")
        }
        SourceRegion::WorkspaceAll => false,
    }));
    assert!(resolved.git_stamp.is_some());
}

#[test]
fn resolve_git_increments_git_counter() {
    let tmp = seed_python();
    super::counters::reset();
    super::resolve::resolve_only(tmp.path(), &req(TargetFocus::Git(GitFocus::Commit))).unwrap();
    assert_eq!(super::counters::current().git, 1);
    assert_eq!(super::counters::current().parse, 0);
}

#[test]
fn resolve_operands_increments_parse_counter() {
    let tmp = seed_python();
    super::counters::reset();
    super::resolve::resolve_only(tmp.path(), &req(ops(&["pkg/app.py"]))).unwrap();
    assert_eq!(super::counters::current().parse, 1);
    assert_eq!(super::counters::current().git, 0);
}
