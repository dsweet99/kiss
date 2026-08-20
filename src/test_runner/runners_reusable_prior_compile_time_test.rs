use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;
use tempfile::TempDir;

use crate::test_runner::coverage_decision::SelectionBasis;
use crate::test_runner::runners::combined_selectors;
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity, rebuild_rust_coverage_index,
    write_rust_population_manifest_for_args, write_test_entry,
};

#[test]
fn build_script_edit_forces_population_while_ordinary_lib_stays_reusable() {
    let tmp = TempDir::new().unwrap();
    let lib = warm_app_builder_workspace(tmp.path());
    fs::write(
        &lib,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 2); } }\n",
    )
    .unwrap();
    let ordinary = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();
    assert!(!ordinary.population_required.rust);
    assert_eq!(ordinary.selection_basis.rust, SelectionBasis::ReusablePrior);

    fs::write(
        tmp.path().join("builder").join("build.rs"),
        "fn main() { println!(\"cargo:rerun-if-env-changed=BUILD_SCRIPT_INPUT\"); }\n",
    )
    .unwrap();
    let compile_time = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();
    assert!(compile_time.population_required.rust);
    assert_eq!(
        compile_time.selection_basis.rust,
        SelectionBasis::Population
    );
}

#[test]
fn manifest_only_compile_time_edit_forces_population() {
    let tmp = TempDir::new().unwrap();
    let _lib = warm_app_builder_workspace(tmp.path());
    let manifest = tmp.path().join("builder").join("Cargo.toml");
    fs::write(
        &manifest,
        "[package]\nname='builder'\nversion='0.1.1'\nedition='2024'\nbuild='build.rs'\n",
    )
    .unwrap();

    let compile_time = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&manifest),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();

    assert!(compile_time.population_required.rust);
    assert_eq!(
        compile_time.selection_basis.rust,
        SelectionBasis::Population
    );
}

#[test]
fn proc_macro_edit_forces_population_while_ordinary_lib_stays_reusable() {
    let tmp = TempDir::new().unwrap();
    let lib = warm_app_proc_macro_workspace(tmp.path());
    fs::write(
        &lib,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 2); } }\n",
    )
    .unwrap();
    let ordinary = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();
    assert!(!ordinary.population_required.rust);
    assert_eq!(ordinary.selection_basis.rust, SelectionBasis::ReusablePrior);

    fs::write(
        tmp.path().join("macros").join("src").join("lib.rs"),
        "extern crate proc_macro;\nuse proc_macro::TokenStream;\n#[proc_macro]\npub fn mark(input: TokenStream) -> TokenStream { input }\n",
    )
    .unwrap();
    let compile_time = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();
    assert!(compile_time.population_required.rust);
    assert_eq!(
        compile_time.selection_basis.rust,
        SelectionBasis::Population
    );
}

fn warm_app_builder_workspace(root: &Path) -> std::path::PathBuf {
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\", \"builder\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("app").join("src")).unwrap();
    fs::create_dir_all(root.join("builder").join("src")).unwrap();
    fs::write(
        root.join("app").join("Cargo.toml"),
        "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("builder").join("Cargo.toml"),
        "[package]\nname='builder'\nversion='0.1.0'\nedition='2024'\nbuild='build.rs'\n",
    )
    .unwrap();
    fs::write(
        root.join("builder").join("build.rs"),
        "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }\n",
    )
    .unwrap();
    fs::write(
        root.join("builder").join("src").join("lib.rs"),
        "pub fn marker() -> u32 { 1 }\n",
    )
    .unwrap();
    warm_app_lib_population(root)
}

fn warm_app_proc_macro_workspace(root: &Path) -> std::path::PathBuf {
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\", \"macros\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("app").join("src")).unwrap();
    fs::create_dir_all(root.join("macros").join("src")).unwrap();
    fs::write(
        root.join("app").join("Cargo.toml"),
        "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("macros").join("Cargo.toml"),
        "[package]\nname='macros'\nversion='0.1.0'\nedition='2024'\n[lib]\nproc-macro = true\n",
    )
    .unwrap();
    fs::write(
        root.join("macros").join("src").join("lib.rs"),
        "extern crate proc_macro;\nuse proc_macro::TokenStream;\n#[proc_macro]\npub fn identity(input: TokenStream) -> TokenStream { input }\n",
    )
    .unwrap();
    warm_app_lib_population(root)
}

fn warm_app_lib_population(root: &Path) -> std::path::PathBuf {
    let lib = root.join("app").join("src").join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 1); } }\n",
    )
    .unwrap();
    let _ = current_rust_coverage_batch_identity(root, &[]);
    write_test_entry(
        root,
        "abc",
        "tests::gets_value",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                "app/src/lib.rs".to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(root).unwrap();
    write_rust_population_manifest_for_args(root, &["tests::gets_value".to_string()], &[]).unwrap();
    lib
}
