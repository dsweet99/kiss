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
fn load_current_population_state_rejects_selection_context_mismatch() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
        value["selection_context_fingerprint"] =
            serde_json::Value::String("wrong-context".to_string());
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

#[test]
fn load_current_population_state_rejects_malformed_source_digest_records() {
    for records in [
        serde_json::json!([{ "path": "/abs.rs", "digest": "aaaaaaaaaaaaaaaa" }]),
        serde_json::json!([{ "path": "../escape.rs", "digest": "aaaaaaaaaaaaaaaa" }]),
        serde_json::json!([{ "path": "src/lib.py", "digest": "aaaaaaaaaaaaaaaa" }]),
        serde_json::json!([{ "path": "src/lib.rs", "digest": "AAAAAAAAAAAAAAAA" }]),
        serde_json::json!([{ "path": "src/lib.rs", "digest": "aaa" }]),
        serde_json::json!([
            { "path": "src/b.rs", "digest": "bbbbbbbbbbbbbbbb" },
            { "path": "src/a.rs", "digest": "aaaaaaaaaaaaaaaa" }
        ]),
        serde_json::json!([
            { "path": "src/a.rs", "digest": "aaaaaaaaaaaaaaaa" },
            { "path": "src/a.rs", "digest": "bbbbbbbbbbbbbbbb" }
        ]),
    ] {
        let fixture = published_alpha_derived_fixture();
        tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
            value["ordinary_source_digests"] = records.clone();
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
}

#[test]
fn load_current_population_state_accepts_inc_source_digest_records() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
        value["ordinary_source_digests"] =
            serde_json::json!([{ "path": "src/fragment.inc", "digest": "aaaaaaaaaaaaaaaa" }]);
    });
    let state = load_current_population_state(
        &fixture.req.cache_root,
        fixture.repo.path(),
        &fixture.identity,
        Some(&["alpha".to_string()]),
    )
    .unwrap();
    assert_eq!(
        state.ordinary_source_digests.get("src/fragment.inc"),
        Some(&"aaaaaaaaaaaaaaaa".to_string())
    );
}

#[test]
fn load_current_population_state_rejects_empty_test_binary_ids() {
    let fixture = published_alpha_derived_fixture();
    tamper_only_cached_entry(&fixture.req.cache_root, |value| {
        value["test_binary_ids"] = serde_json::json!([]);
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

fn tamper_only_cached_entry(
    cache_root: &std::path::Path,
    edit: impl FnOnce(&mut serde_json::Value),
) {
    let entries_dir = cache_root.join("entries");
    let paths = std::fs::read_dir(entries_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 1);
    let relative = paths[0].strip_prefix(cache_root).unwrap().to_string_lossy();
    tamper_json_file(cache_root, &relative, edit);
}
