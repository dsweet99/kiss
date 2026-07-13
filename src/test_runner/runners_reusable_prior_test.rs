use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;
use tempfile::TempDir;

use crate::test_runner::coverage_decision::{CoverageFreshness, LanguagePlanner, RustSelectionBasis};
use crate::test_runner::runners::rust_backer::RustModule;
use crate::test_runner::runners::{combined_selectors, enumerate_workspace_rust_selectors};
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity, rebuild_rust_coverage_index,
    resolve_rust_population_state, write_rust_population_manifest_for_args, write_test_entry,
};

#[test]
fn reusable_prior_real_cache_fixture() {
    if std::env::var_os("KISS_REUSABLE_PRIOR_REAL_CACHE").is_none() {
        return;
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let population = repo.join(".kiss/rust_llvm_cov_cache/population.json");
    assert!(population.is_file(), "expected warm .kiss population");
    let manifest =
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&population).unwrap())
            .unwrap();
    assert_eq!(manifest["schema_version"], "rust-llvm-cov-population-v3");
    let cli = repo.join("src/cli_output.rs");
    let plan = combined_selectors(
        repo,
        std::slice::from_ref(&cli),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .expect("combined selectors");
    let universe = enumerate_workspace_rust_selectors(repo, &[]).unwrap();
    assert_eq!(
        resolve_rust_population_state(repo, &[], std::slice::from_ref(&cli), &[])
            .expect("resolved")
            .freshness,
        CoverageFreshness::ReusablePrior
    );
    assert!(!plan.rust_population_required);
    assert_eq!(plan.rust_selection_basis, RustSelectionBasis::ReusablePrior);
    assert!(!plan.rust_selectors.is_empty());
    assert!(plan.rust_selectors.len() < universe.len());
}

#[test]
fn combined_selectors_uses_reusable_prior_after_ordinary_source_edit() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = src.join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 1); } }\n",
    )
    .unwrap();
    let _ = current_rust_coverage_batch_identity(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "abc",
        "tests::gets_value",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                "src/lib.rs".to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_population_manifest_for_args(tmp.path(), &["tests::gets_value".to_string()], &[])
        .unwrap();
    fs::write(
        &lib,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 2); } }\n",
    )
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
    assert!(!plan.rust_population_required);
    assert_eq!(plan.rust_selection_basis, RustSelectionBasis::ReusablePrior);
}

#[test]
fn rust_module_reports_reusable_prior_after_ordinary_source_edit() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)] mod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    let _ = current_rust_coverage_batch_identity(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "value",
        "tests::gets_value",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                lib.to_string_lossy().to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_population_manifest_for_args(tmp.path(), &["tests::gets_value".to_string()], &[])
        .unwrap();
    fs::write(
        &lib,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)] mod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();

    let module = RustModule::new(
        tmp.path(),
        std::slice::from_ref(&lib),
        &BTreeMap::new(),
        &[],
        &[],
        &[],
        &[],
    );
    let universe = module.discover_universe().unwrap();
    assert_eq!(
        <RustModule as LanguagePlanner>::freshness(&module, &universe).unwrap(),
        CoverageFreshness::ReusablePrior
    );
    assert_eq!(
        module.rust_selection_basis().unwrap(),
        RustSelectionBasis::ReusablePrior
    );
    let selection = <RustModule as LanguagePlanner>::select(&module).unwrap();
    assert!(selection.complete);
    assert_eq!(
        selection
            .selectors
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        vec!["tests::gets_value"]
    );
    assert_eq!(
        module.selection_basis().unwrap(),
        RustSelectionBasis::ReusablePrior
    );
    let empty_module = RustModule::new(tmp.path(), &[], &BTreeMap::new(), &[], &[], &[], &[]);
    assert_eq!(
        empty_module.selection_basis().unwrap(),
        RustSelectionBasis::Current
    );
}

#[test]
fn cargo_toml_invalidator_forces_population_after_warm_snapshot() {
    let tmp = TempDir::new().unwrap();
    let lib = warm_demo_repo(tmp.path());
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.1'\nedition='2024'\n",
    )
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
    assert!(plan.rust_population_required);
    assert_eq!(plan.rust_selection_basis, RustSelectionBasis::Population);
}

#[test]
fn corrupt_prior_index_row_forces_population() {
    let tmp = TempDir::new().unwrap();
    let lib = warm_demo_repo(tmp.path());
    fs::write(
        &lib,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 2); } }\n",
    )
    .unwrap();
    let index_path = tmp
        .path()
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("index.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&index_path).unwrap()).unwrap();
    if let Some(files) = value.get_mut("files") {
        *files = serde_json::json!({});
    }
    fs::write(&index_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
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
    assert!(plan.rust_population_required);
    assert_eq!(plan.rust_selection_basis, RustSelectionBasis::Population);
}

#[test]
fn renamed_production_rs_path_forces_population() {
    let tmp = TempDir::new().unwrap();
    let lib = warm_demo_repo(tmp.path());
    let renamed = tmp.path().join("src").join("renamed.rs");
    fs::rename(&lib, &renamed).unwrap();
    let plan = combined_selectors(
        tmp.path(),
        &[lib, renamed],
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();
    assert!(plan.rust_population_required);
    assert_eq!(plan.rust_selection_basis, RustSelectionBasis::Population);
}

fn warm_demo_repo(root: &Path) -> std::path::PathBuf {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = root.join("src").join("lib.rs");
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
                "src/lib.rs".to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(root).unwrap();
    write_rust_population_manifest_for_args(root, &["tests::gets_value".to_string()], &[]).unwrap();
    lib
}
