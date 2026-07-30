use crate::batch_derived::POPULATION_SCHEMA_VERSION;
use crate::batch_derived_index::{
    load_current_population_state, load_reusable_prior_population_state,
};
use crate::test_support::{published_alpha_derived_fixture, tamper_json_file};

#[test]
fn strict_loader_rejects_source_digest_mismatch_while_reusable_accepts() {
    let fixture = published_alpha_derived_fixture();
    let mut stale_identity = fixture.identity.clone();
    stale_identity.input_digest = "stale-input".to_string();
    stale_identity.generation_fingerprint = "stale-generation".to_string();
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &stale_identity,
            Some(&["alpha".to_string()]),
        )
        .is_none()
    );
    assert!(
        load_reusable_prior_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            Some(&["alpha".to_string()]),
            &fixture.identity.selection_context_fingerprint,
        )
        .is_some()
    );
}

#[test]
fn reusable_loader_rejects_selection_context_mismatch() {
    let fixture = published_alpha_derived_fixture();
    assert!(
        load_reusable_prior_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            Some(&["alpha".to_string()]),
            "wrong-context",
        )
        .is_none()
    );
}

#[test]
fn reusable_loader_rejects_tampered_index_files() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "index.json", |value| {
        if let Some(files) = value.get_mut("files") {
            *files = serde_json::json!({});
        }
    });
    assert!(
        load_reusable_prior_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            Some(&["alpha".to_string()]),
            &fixture.identity.selection_context_fingerprint,
        )
        .is_none()
    );
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&["alpha".to_string()]),
        )
        .is_none()
    );
}

#[test]
fn v4_manifest_is_unavailable_for_reusable_loading() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
        value["schema_version"] =
            serde_json::Value::String("rust-llvm-cov-population-v4".to_string());
    });
    assert!(
        load_reusable_prior_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            Some(&["alpha".to_string()]),
            &fixture.identity.selection_context_fingerprint,
        )
        .is_none()
    );
    assert_eq!(POPULATION_SCHEMA_VERSION, "rust-llvm-cov-population-v6");
}
