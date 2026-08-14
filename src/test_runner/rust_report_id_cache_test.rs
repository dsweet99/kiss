use super::*;
use std::fs;

#[test]
fn report_id_cache_round_trip_hits_without_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // First call may miss and build (or yield empty if parse path differs); store explicitly.
    let mut map = BTreeMap::new();
    map.insert("tests::t".into(), "src/lib.rs::t".into());
    store_cached(root, &[], &map).unwrap();
    let loaded = try_load_cached(root, &[]).expect("cache hit");
    assert_eq!(loaded.get("tests::t").map(String::as_str), Some("src/lib.rs::t"));
    let via = rust_logical_to_kiss_test_ids_cached(root, &[]).unwrap();
    assert_eq!(via.get("tests::t").map(String::as_str), Some("src/lib.rs::t"));
}
