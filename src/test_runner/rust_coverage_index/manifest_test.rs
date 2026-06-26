use std::collections::BTreeMap;

use super::{
    CACHE_SCHEMA_VERSION, POPULATION_SCHEMA_VERSION, RUST_SELECTOR_DISCOVERY_VERSION,
    RustPopulationManifest, RustPopulationManifestIdentity,
};

fn test_identity() -> RustPopulationManifestIdentity {
    RustPopulationManifestIdentity {
        cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: RUST_SELECTOR_DISCOVERY_VERSION.to_string(),
        rustc_version: "rustc".to_string(),
        cargo_version: "cargo".to_string(),
        cargo_llvm_cov_version: "llvm-cov".to_string(),
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
    }
}

#[test]
fn rust_population_manifest_identity() {
    let identity = test_identity();

    assert!(identity.has_tool_versions());
    assert_eq!(identity.tool_versions(), ["rustc", "cargo", "llvm-cov"]);
    assert!(identity.args_match(&[], &[]));
    assert!(std::mem::size_of::<RustPopulationManifestIdentity>() > 0);
}

#[test]
fn rust_population_manifest() {
    let identity = test_identity();
    let manifest = RustPopulationManifest {
        schema_version: POPULATION_SCHEMA_VERSION.to_string(),
        cache_schema_version: identity.cache_schema_version.clone(),
        source_root: "root".to_string(),
        selector_discovery_version: identity.selector_discovery_version.clone(),
        rustc_version: identity.rustc_version.clone(),
        cargo_version: identity.cargo_version.clone(),
        cargo_llvm_cov_version: identity.cargo_llvm_cov_version.clone(),
        cargo_args: identity.cargo_args.clone(),
        test_args: identity.test_args.clone(),
        env: identity.env.clone(),
        input_fingerprint: "input".to_string(),
        entries_fingerprint: "entries".to_string(),
        selectors: vec!["test_lib".to_string()],
    };

    assert!(manifest.matches_identity(&identity, "root"));
    assert!(manifest.matches_selectors(&["test_lib".to_string()]));
    assert!(std::mem::size_of::<RustPopulationManifest>() > 0);
}
