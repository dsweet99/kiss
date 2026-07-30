use crate::batch_entry_state::publish_next_entry_state;
use crate::batch_reverse_build::BuiltReverseIndex;
use crate::batch_reverse_publish::{
    prune_unreferenced_snapshots, snapshot_path, write_reverse_snapshot,
};
use std::collections::BTreeMap;
use tempfile::tempdir;

#[test]
fn prune_retains_active_and_prior_snapshots() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path();
    let built = BuiltReverseIndex {
        selectors: vec!["a".into()],
        files: BTreeMap::new(),
    };
    let r1 = publish_next_entry_state(cache, "gen", "fp1").unwrap();
    let s1 = write_reverse_snapshot(cache, "gen", "fp1", r1, &built).unwrap();
    let r2 = publish_next_entry_state(cache, "gen", "fp2").unwrap();
    let s2 = write_reverse_snapshot(cache, "gen", "fp2", r2, &built).unwrap();
    let r3 = publish_next_entry_state(cache, "gen", "fp3").unwrap();
    let s3 = write_reverse_snapshot(cache, "gen", "fp3", r3, &built).unwrap();
    let removed =
        prune_unreferenced_snapshots(cache, &s3.snapshot_id, Some(&s2.snapshot_id)).unwrap();
    assert!(removed >= 1);
    assert!(snapshot_path(cache, &s3.snapshot_id).is_dir());
    assert!(snapshot_path(cache, &s2.snapshot_id).is_dir());
    assert!(!snapshot_path(cache, &s1.snapshot_id).exists());
}
