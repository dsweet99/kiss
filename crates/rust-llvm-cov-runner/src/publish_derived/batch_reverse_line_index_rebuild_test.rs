use crate::publish_derived::batch_derived_index::load_current_population_state;
use crate::publish_derived::batch_entry_state::publish_next_entry_state;
use crate::publish_derived::batch_reverse_publish::{
    publish_reverse_line_index, reverse_line_index_dir, snapshot_path,
};
use crate::publish_derived::batch_reverse_test_support::seed_alpha_beta_reverse;
use crate::test_support::{
    batch_executor_fixture_repo, batch_executor_request, published_alpha_derived_fixture,
    store_batch_executor_selector, witness_batch_tools,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn rebuild_workspace_reverse_line_index() {
    let fixture = published_alpha_derived_fixture();
    let cache = &fixture.req.cache_root;
    let generation = &fixture.identity.generation_fingerprint;
    let population: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("population.json")).unwrap()).unwrap();
    let entries_fp = population["entries_fingerprint"].as_str().unwrap();
    let revision = publish_next_entry_state(cache, generation, entries_fp).unwrap();
    let info =
        publish_reverse_line_index(cache, fixture.repo.path(), generation, entries_fp, revision)
            .unwrap();
    assert!(
        snapshot_path(cache, &info.snapshot_id)
            .join("meta.json")
            .is_file()
    );
    assert!(reverse_line_index_dir(cache).join("snapshots").is_dir());
}

#[test]
fn population_load_skips_entry_scan_when_reverse_bound() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let _ = seed_alpha_beta_reverse(&req);
    let tools = witness_batch_tools();
    let identity = crate::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let entries = req.cache_root.join("entries");
    let mode = entries.metadata().unwrap().permissions().mode();
    fs::set_permissions(&entries, fs::Permissions::from_mode(0o000)).unwrap();
    let state = load_current_population_state(&req.cache_root, repo.path(), &identity, None);
    fs::set_permissions(&entries, fs::Permissions::from_mode(mode)).unwrap();
    assert!(
        state.is_some(),
        "bound reverse+entry_state must load without reading entries"
    );
}
