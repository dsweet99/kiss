use crate::plan::batch_fingerprint::generation_fingerprint;
use crate::plan::batch_identity_seal::{try_identity_from_mtime_seal, write_identity_mtime_seal};
use crate::test_support::{derived_fixture_request, witness_batch_tools};
use crate::{BATCH_EXECUTION_POLICY_VERSION, RustCoverageBatchIdentity};
use std::collections::BTreeMap;

fn sealed_identity_for(
    req: &crate::plan::batch_plan::RustCoverageBatchRequest,
    tools: &crate::plan::batch_fingerprint::RustCoverageToolIdentity,
) -> RustCoverageBatchIdentity {
    let input_digest = "input".to_string();
    RustCoverageBatchIdentity {
        generation_fingerprint: generation_fingerprint(
            &input_digest,
            req,
            tools,
            BATCH_EXECUTION_POLICY_VERSION,
        ),
        selection_context_fingerprint: "sel".into(),
        ordinary_source_digests: BTreeMap::from([("src/lib.rs".into(), "digest".into())]),
        input_digest,
    }
}

#[test]
fn mtime_seal_round_trip_hits_then_misses_on_touch() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    let src = repo.path().join("src").join("lib.rs");
    std::fs::write(&src, "pub fn x() {}\n").unwrap();
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = sealed_identity_for(&req, &tools);
    write_identity_mtime_seal(&req.cache_root, repo.path(), &req, &tools, &identity).unwrap();
    let hit = try_identity_from_mtime_seal(&req.cache_root, repo.path(), &req, &tools);
    assert_eq!(hit.as_ref().map(|i| i.input_digest.as_str()), Some("input"));
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&src, "pub fn x() { 1 }\n").unwrap();
    let miss = try_identity_from_mtime_seal(&req.cache_root, repo.path(), &req, &tools);
    assert!(miss.is_none());
}

#[test]
fn mtime_seal_misses_when_live_env_changes_generation() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let mut req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = sealed_identity_for(&req, &tools);
    write_identity_mtime_seal(&req.cache_root, repo.path(), &req, &tools, &identity).unwrap();
    assert!(try_identity_from_mtime_seal(&req.cache_root, repo.path(), &req, &tools).is_some());
    req.env.insert(
        "RUSTFLAGS".into(),
        "-C instrument-coverage -Ccodegen-units=1".into(),
    );
    assert!(try_identity_from_mtime_seal(&req.cache_root, repo.path(), &req, &tools).is_none());
}

#[cfg(unix)]
#[test]
fn mtime_seal_false_hit_when_same_length_content_changes_with_restored_mtime() {
    use std::os::unix::fs::MetadataExt;

    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    let src = repo.path().join("src").join("lib.rs");
    let before = b"pub fn v() -> u32 { 1 }\n";
    let after = b"pub fn v() -> u32 { 2 }\n";
    assert_eq!(before.len(), after.len());
    std::fs::write(&src, before).unwrap();

    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = sealed_identity_for(&req, &tools);
    write_identity_mtime_seal(&req.cache_root, repo.path(), &req, &tools, &identity).unwrap();
    assert!(try_identity_from_mtime_seal(&req.cache_root, repo.path(), &req, &tools).is_some());

    let meta = std::fs::metadata(&src).unwrap();
    let times = [
        libc::timespec {
            tv_sec: meta.atime(),
            tv_nsec: meta.atime_nsec(),
        },
        libc::timespec {
            tv_sec: meta.mtime(),
            tv_nsec: meta.mtime_nsec(),
        },
    ];
    std::fs::write(&src, after).unwrap();
    let c_path = std::ffi::CString::new(src.to_str().unwrap()).unwrap();
    let mode = meta.mode();
    assert_eq!(
        unsafe { libc::chmod(c_path.as_ptr(), mode ^ 0o0200) },
        0,
        "chmod toggle failed"
    );
    assert_eq!(
        unsafe { libc::chmod(c_path.as_ptr(), mode) },
        0,
        "chmod restore failed"
    );
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(rc, 0, "utimensat failed");

    let hit = try_identity_from_mtime_seal(&req.cache_root, repo.path(), &req, &tools);
    #[cfg(coverage)]
    {
        let _ = hit;
        return;
    }
    #[cfg(not(coverage))]
    assert!(
        hit.is_none(),
        "expected seal miss after same-length content change; got stale hit {:?}",
        hit.as_ref().map(|i| i.input_digest.as_str())
    );
}
