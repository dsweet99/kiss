use crate::test_runner::runners::python_backer::{PythonBackerInput, PythonModule};
use crate::test_runner::runners::rust_backer::{RustBackerInput, RustModule};
use crate::test_runner::test_selection::{LanguagePlanner, SelectionBasis};

fn python_basis(
    repo_root: &std::path::Path,
    source_paths: &[std::path::PathBuf],
) -> SelectionBasis {
    PythonModule::new(PythonBackerInput {
        repo_root,
        py_source_paths: source_paths,
        test_args: &[],
        ignore: &[],
        changed_tests: &[],
        prior_failures: &[],
    })
    .selection_basis()
}

fn rust_basis(repo_root: &std::path::Path, source_paths: &[std::path::PathBuf]) -> SelectionBasis {
    RustModule::new(RustBackerInput {
        repo_root,
        rust_source_paths: source_paths,
        ignore: &[],
        changed_tests: &[],
        prior_failures: &[],
    })
    .selection_basis()
}

#[test]
fn concrete_language_planners_keep_policy_parity() {
    let tmp = tempfile::TempDir::new().unwrap();
    let app = tmp.path().join("app.py");
    let lib = tmp.path().join("src").join("lib.rs");

    assert_eq!(python_basis(tmp.path(), &[]), SelectionBasis::Current);
    assert_eq!(rust_basis(tmp.path(), &[]), SelectionBasis::Current);
    assert_eq!(
        python_basis(tmp.path(), std::slice::from_ref(&app)),
        SelectionBasis::Population
    );
    assert_eq!(
        rust_basis(tmp.path(), std::slice::from_ref(&lib)),
        SelectionBasis::Population
    );
}
