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

    let mut map = BTreeMap::new();
    map.insert("tests::t".into(), "src/lib.rs::t".into());
    store_cached(root, &[], &map).unwrap();
    let loaded = try_load_cached(root, &[]).expect("cache hit");
    assert_eq!(
        loaded.get("tests::t").map(String::as_str),
        Some("src/lib.rs::t")
    );
    let via = rust_logical_to_kiss_test_ids_cached(root, &[]).unwrap();
    assert_eq!(
        via.get("tests::t").map(String::as_str),
        Some("src/lib.rs::t")
    );
}

#[test]
fn report_id_cache_misses_when_rust_source_mtime_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    let lib = root.join("src/lib.rs");
    fs::write(
        &lib,
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();

    let mut map = BTreeMap::new();
    map.insert("tests::t".into(), "src/lib.rs::t".into());
    store_cached(root, &[], &map).unwrap();
    assert!(try_load_cached(root, &[]).is_some());

    fs::write(
        &lib,
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n    #[test]\n    fn u() {}\n}\n",
    )
    .unwrap();
    assert!(
        try_load_cached(root, &[]).is_none(),
        "source edit must invalidate rust report-id cache"
    );
}

#[test]
fn report_id_cache_ignores_python_only_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "#[test]\nfn t() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("app.py"), "VALUE = 1\n").unwrap();
    let map = BTreeMap::from([("t".into(), "src/lib.rs::t".into())]);
    store_cached(root, &[], &map).unwrap();
    fs::write(root.join("app.py"), "VALUE = 2\n").unwrap();
    assert_eq!(try_load_cached(root, &[]), Some(map));
}

#[test]
fn in_process_report_id_memo_observes_rust_fingerprint_change() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    let lib = root.join("src/lib.rs");
    fs::write(&lib, "#[test]\nfn t() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    let old = BTreeMap::from([("t".into(), "src/lib.rs::t".into())]);
    store_cached(root, &[], &old).unwrap();
    assert_eq!(
        rust_logical_to_kiss_test_ids_cached(root, &[]).unwrap(),
        old
    );

    fs::write(&lib, "#[test]\nfn u() {}\n").unwrap();
    let new = BTreeMap::from([("u".into(), "src/lib.rs::u".into())]);
    store_cached(root, &[], &new).unwrap();
    assert_eq!(
        rust_logical_to_kiss_test_ids_cached(root, &[]).unwrap(),
        new
    );
}
