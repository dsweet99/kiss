use super::*;

#[test]
fn select_fresh_python_source_selectors_and_select_fresh_rust_source_selectors_changed_line_coverage()
 {
    let tmp = tempfile::TempDir::new().unwrap();
    let app = tmp.path().join("app.py");
    let lib = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    std::fs::write(&app, "def value():\n    return 1\n").unwrap();
    std::fs::write(
        &lib,
        "pub fn value() -> i32 { 1 }\n#[cfg(test)] mod tests { #[test] fn test_value() {} }\n",
    )
    .unwrap();
    write_python_entry(
        tmp.path(),
        "py",
        "tests/test_app.py::test_value",
        LineCoverage {
            files: BTreeMap::from([(app.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_rust_entry(
        tmp.path(),
        "rs",
        "tests::test_value",
        rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    rebuild_python_coverage_index(tmp.path()).unwrap();
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_test_population(tmp.path(), "tests::test_value");

    assert_eq!(
        python_backer::select_fresh_python_source_selectors(
            tmp.path(),
            std::slice::from_ref(&app),
            &single_line_change(&app),
        ),
        Some(BTreeSet::from(
            ["tests/test_app.py::test_value".to_string()]
        ))
    );
    assert_eq!(
        select_fresh_rust_source_selectors(
            tmp.path(),
            std::slice::from_ref(&lib),
            &single_line_change(&lib),
            &[],
        ),
        Some(BTreeSet::from(["tests::test_value".to_string()]))
    );

    assert_python_module_selects(tmp.path(), &app);
    assert_rust_module_selects(tmp.path(), &lib);
}

fn assert_python_module_selects(repo: &std::path::Path, app: &std::path::Path) {
    let python = python_backer::PythonModule::new(
        repo,
        std::slice::from_ref(&app.to_path_buf()),
        &single_line_change(app),
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        <python_backer::PythonModule as LanguagePlanner>::select(&python).unwrap(),
        SelectionDecision {
            selectors: vec![TestSelector::new(
                kiss::Language::Python,
                "tests/test_app.py::test_value"
            )],
            complete: true,
        }
    );
}

fn assert_rust_module_selects(repo: &std::path::Path, lib: &std::path::Path) {
    let rust = RustModule::new(
        repo,
        std::slice::from_ref(&lib.to_path_buf()),
        &single_line_change(lib),
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        <RustModule as LanguagePlanner>::select(&rust).unwrap(),
        SelectionDecision {
            selectors: vec![TestSelector::new(kiss::Language::Rust, "tests::test_value")],
            complete: true,
        }
    );
}

fn write_rust_test_population(repo: &std::path::Path, selector: &str) {
    write_rust_population_manifest_for_args(repo, &[selector.to_string()], &[]).unwrap();
}

fn single_line_change(path: &std::path::Path) -> BTreeMap<std::path::PathBuf, BTreeSet<u32>> {
    BTreeMap::from([(path.to_path_buf(), BTreeSet::from([1]))])
}
