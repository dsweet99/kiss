use super::*;
use tempfile::tempdir;

#[test]
fn sweep_removes_entry_state_tmp_and_nested_tmps() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path();
    let nested = cache.join("reverse_line_index").join("snapshots");
    fs::create_dir_all(&nested).unwrap();
    let entry_tmp = cache.join(".entry_state.1.2.tmp");
    let nested_tmp = nested.join(".meta.json.3.4.tmp");
    let keep = cache.join("entry_state.json");
    fs::write(&entry_tmp, b"{}\n").unwrap();
    fs::write(&nested_tmp, b"{}\n").unwrap();
    fs::write(&keep, b"{}\n").unwrap();

    sweep_orphaned_publication_tmps(cache).unwrap();

    assert!(!entry_tmp.exists());
    assert!(!nested_tmp.exists());
    assert!(keep.exists());
}

#[test]
fn sweep_missing_cache_root_is_ok() {
    let tmp = tempdir().unwrap();
    sweep_orphaned_publication_tmps(&tmp.path().join("missing")).unwrap();
}
