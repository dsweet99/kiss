use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::test_runner::coverage_decision::{CoverageFreshness, LanguagePlanner, SelectionBasis};
use crate::test_runner::runners::rust_backer::RustModule;
use crate::test_runner::runners::{combined_selectors, enumerate_workspace_rust_selectors};
use crate::test_runner::rust_coverage_index::{
    ResolveRustPopulationArgs, resolve_rust_population_state,
};
use crate::test_runner::test_mode_fixtures::clone_warm_demo_repo;
use crate::test_runner::test_mode_fixtures::with_locked_warm_demo_repo;

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
    assert_eq!(manifest["schema_version"], "rust-llvm-cov-population-v4");
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
        resolve_rust_population_state(ResolveRustPopulationArgs::for_paths(
            repo,
            std::slice::from_ref(&cli),
        ))
        .expect("resolved")
        .freshness(),
        CoverageFreshness::ReusablePrior
    );
    assert!(!plan.population_required.rust);
    assert_eq!(plan.selection_basis.rust, SelectionBasis::ReusablePrior);
    assert!(!plan.selectors.rust.is_empty());
    assert!(plan.selectors.rust.len() < universe.len());
}

#[test]
fn combined_selectors_uses_reusable_prior_after_ordinary_source_edit() {
    with_locked_warm_demo_repo(|repo, lib| {
        let plan = combined_selectors(
            repo,
            std::slice::from_ref(&lib),
            &[],
            &BTreeMap::new(),
            &[],
            None,
            &[],
        )
        .unwrap();

        assert_eq!(plan.selectors.rust, vec!["tests::gets_value".to_string()]);
        assert!(!plan.population_required.rust);
        assert_eq!(plan.selection_basis.rust, SelectionBasis::ReusablePrior);
    });
}

#[test]
fn reusable_prior_uses_snapshot_delta_instead_of_historical_vcs_sources() {
    crate::test_runner::test_mode_fixtures::with_locked_base_historical_repo(
        |repo, _baseline, lib| {
            let historical = repo.join("src").join("historical.rs");
            let plan = combined_selectors(
                repo,
                &[historical, lib.clone()],
                &[],
                &BTreeMap::new(),
                &[],
                None,
                &[],
            )
            .unwrap();

            assert!(!plan.population_required.rust);
            assert_eq!(plan.selection_basis.rust, SelectionBasis::ReusablePrior);
            assert_eq!(plan.source_paths.rust, vec![lib]);
            assert_eq!(plan.vcs_source_paths.rust, 2);
            assert_eq!(plan.snapshot_delta_modified.rust, 1);
            assert!(!plan.snapshot_delta_structural.rust);
            assert_eq!(plan.selectors.rust, vec!["tests::gets_value".to_string()]);
        },
    );
}

#[test]
fn rust_module_reports_reusable_prior_after_ordinary_source_edit() {
    with_locked_warm_demo_repo(|repo, lib| {
        let module = RustModule::new(
            repo,
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
        assert_eq!(module.selection_basis(), SelectionBasis::ReusablePrior);
        let empty_module = RustModule::new(repo, &[], &BTreeMap::new(), &[], &[], &[], &[]);
        assert_eq!(empty_module.selection_basis(), SelectionBasis::Current);
    });
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
    assert!(plan.population_required.rust);
    assert_eq!(plan.selection_basis.rust, SelectionBasis::Population);
}

#[test]
fn corrupt_prior_index_row_forces_population() {
    let tmp = TempDir::new().unwrap();
    let lib = clone_warm_demo_repo(tmp.path());
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
    assert!(plan.population_required.rust);
    assert_eq!(plan.selection_basis.rust, SelectionBasis::Population);
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
    assert!(plan.population_required.rust);
    assert_eq!(plan.selection_basis.rust, SelectionBasis::Population);
}

fn warm_demo_repo(root: &Path) -> std::path::PathBuf {
    clone_warm_demo_repo(root)
}
