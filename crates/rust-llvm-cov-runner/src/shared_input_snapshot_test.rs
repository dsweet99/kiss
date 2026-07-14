use std::collections::BTreeMap;
use std::fs;

use super::shared_input_snapshot::{
    RustInputSnapshot, digest_input_file_snapshot, rust_input_snapshot,
};

#[test]
fn rust_input_snapshot_tracks_ordinary_sources_without_selection_context_churn() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let mut req = crate::batch_plan::RustCoverageBatchRequest::witness();
    req.cwd = tmp.path().to_path_buf();
    req.source_root = tmp.path().to_path_buf();
    req.cargo_args.clear();

    let before = rust_input_snapshot(tmp.path(), &req).unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
    let after = rust_input_snapshot(tmp.path(), &req).unwrap();

    assert_ne!(before.input_digest, after.input_digest);
    assert_eq!(
        before.selection_context_source_digest,
        after.selection_context_source_digest
    );
    assert_eq!(
        before.ordinary_source_digests.keys().collect::<Vec<_>>(),
        vec![&"src/lib.rs".to_string()]
    );
    assert_ne!(
        before.ordinary_source_digests.get("src/lib.rs"),
        after.ordinary_source_digests.get("src/lib.rs")
    );
}

#[test]
fn rust_input_snapshot_tracks_inc_sources_without_selection_context_churn() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(tmp.path().join("Cargo.lock"), "# lock\n").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "include!(\"part.inc\");\n").unwrap();
    fs::write(tmp.path().join("src").join("part.inc"), "pub fn x() {}\n").unwrap();
    let mut req = crate::batch_plan::RustCoverageBatchRequest::witness();
    req.cwd = tmp.path().to_path_buf();
    req.source_root = tmp.path().to_path_buf();
    req.cargo_args.clear();

    let before = rust_input_snapshot(tmp.path(), &req).unwrap();
    fs::write(tmp.path().join("src").join("part.inc"), "pub fn y() {}\n").unwrap();
    let after = rust_input_snapshot(tmp.path(), &req).unwrap();

    assert_ne!(before.input_digest, after.input_digest);
    assert_eq!(
        before.selection_context_source_digest,
        after.selection_context_source_digest
    );
    assert!(before.ordinary_source_digests.contains_key("src/part.inc"));
    assert_ne!(
        before.ordinary_source_digests.get("src/part.inc"),
        after.ordinary_source_digests.get("src/part.inc")
    );
}

#[test]
fn rust_input_snapshot_keeps_compile_time_rs_out_of_ordinary_map() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\nbuild='build.rs'\n",
    )
    .unwrap();
    fs::write(tmp.path().join("build.rs"), "fn main() {}\n").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let mut req = crate::batch_plan::RustCoverageBatchRequest::witness();
    req.cwd = tmp.path().to_path_buf();
    req.source_root = tmp.path().to_path_buf();
    req.cargo_args.clear();

    let snapshot = rust_input_snapshot(tmp.path(), &req).unwrap();

    assert!(snapshot.ordinary_source_digests.contains_key("src/lib.rs"));
    assert!(!snapshot.ordinary_source_digests.contains_key("build.rs"));
    assert!(!snapshot.selection_context_source_digest.is_empty());
}

#[test]
fn input_snapshot_rejects_duplicate_normalized_ordinary_sources() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn x() {}\n").unwrap();
    let err = digest_input_file_snapshot(
        tmp.path(),
        &[lib.clone(), lib],
        |path| fs::read(path),
        |_| Ok(true),
    )
    .unwrap_err();
    assert!(format!("{err:?}").contains("duplicate ordinary Rust source path"));
}

#[test]
fn input_snapshot_rejects_out_of_repo_ordinary_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), "pub fn x() {}\n").unwrap();
    let err = digest_input_file_snapshot(
        tmp.path(),
        &[outside.path().to_path_buf()],
        |path| fs::read(path),
        |_| Ok(true),
    )
    .unwrap_err();
    assert!(format!("{err:?}").contains("not repository-relative"));
}

#[test]
fn input_snapshot_derive_witness_reads_all_fields() {
    let mut ordinary_source_digests = BTreeMap::new();
    ordinary_source_digests.insert("src/lib.rs".to_string(), "aaaaaaaaaaaaaaaa".to_string());
    let snapshot = RustInputSnapshot {
        input_digest: "input".to_string(),
        selection_context_source_digest: "context".to_string(),
        ordinary_source_digests,
    };
    let cloned = snapshot.clone();
    assert_eq!(snapshot, cloned);
    assert!(format!("{snapshot:?}").contains("src/lib.rs"));
}
