use crate::batch_derived::POPULATION_SCHEMA_VERSION;
use crate::batch_derived_index::load_current_population_state;
use crate::test_support::{published_alpha_derived_fixture, tamper_json_file};

#[test]
fn load_current_population_state_rejects_mismatched_source_root() {
    let fixture = published_alpha_derived_fixture();
    let other = tempfile::tempdir().unwrap();
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            other.path(),
            &fixture.identity,
            Some(&["alpha".to_string()]),
        )
        .is_none()
    );
}

#[test]
fn load_current_population_state_rejects_stale_manifest_fields() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
        value["schema_version"] = serde_json::Value::String("wrong".to_string());
    });
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&["alpha".to_string()]),
        )
        .is_none()
    );

    tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
        value["schema_version"] = serde_json::Value::String(POPULATION_SCHEMA_VERSION.to_string());
        value["generation_fingerprint"] = serde_json::Value::String("wrong".to_string());
    });
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&["alpha".to_string()]),
        )
        .is_none()
    );

    tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
        value["generation_fingerprint"] =
            serde_json::Value::String(fixture.identity.generation_fingerprint.clone());
        value["input_fingerprint"] = serde_json::Value::String("wrong".to_string());
    });
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
fn load_current_population_state_rejects_index_fingerprint_mismatch() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "index.json", |value| {
        value["entries_fingerprint"] = serde_json::Value::String("wrong".to_string());
    });
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&["alpha".to_string()]),
        )
        .is_none()
    );

    tamper_json_file(&fixture.req.cache_root, "index.json", |value| {
        value["schema_version"] = serde_json::Value::String("wrong".to_string());
    });
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
