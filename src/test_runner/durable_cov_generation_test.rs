use std::fs;

use tempfile::tempdir;

use super::{publish_durable_generation, try_hydrate_if_kiss_absent};
use crate::test_runner::check_line_coverage::RequiredCoverageLanguages;

#[test]
fn hydrate_is_noop_when_kiss_already_exists() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".kiss")).unwrap();
    fs::write(repo.join(".kiss/marker"), b"keep").unwrap();
    let required = RequiredCoverageLanguages {
        python: false,
        rust: false,
    };
    assert!(!try_hydrate_if_kiss_absent(repo, required, &[]));
    assert_eq!(
        fs::read_to_string(repo.join(".kiss/marker")).unwrap(),
        "keep"
    );
}

#[test]
fn publish_and_hydrate_round_trip_restores_selected_artifacts() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cache_home = tmp.path().join("cache");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&cache_home).unwrap();
    let _cache_guard = crate::test_runner::TestEnvVarGuard::set(
        "XDG_CACHE_HOME",
        cache_home.to_str().unwrap(),
    );
    let kiss = repo.join(".kiss");
    fs::create_dir_all(kiss.join("rslip_cache/hosts")).unwrap();
    fs::write(kiss.join("rslip_cache/hosts/x"), b"py").unwrap();
    fs::write(kiss.join("cov_records_cache.json"), br#"{"records":[]}"#).unwrap();
    // Rust artifacts present on disk but not required: must not be published when
    // the lease is Python-only (avoids needing a cargo workspace for the key).
    fs::create_dir_all(kiss.join("rust_llvm_cov_cache/entries")).unwrap();
    fs::write(
        kiss.join("rust_llvm_cov_cache/check_aggregate.json"),
        br#"{"ok":true}"#,
    )
    .unwrap();
    fs::write(
        kiss.join("rust_llvm_cov_cache/entries/skip-me.json"),
        br#"{"heavy":true}"#,
    )
    .unwrap();

    let required = RequiredCoverageLanguages {
        python: true,
        rust: false,
    };
    publish_durable_generation(&repo, required, &[]);
    assert!(
        cache_home.join("kiss").join("kiss-cov-durable").is_dir(),
        "durable lease must live under XDG_CACHE_HOME, not the repo tree"
    );
    assert!(
        !repo.join("target").join("kiss-cov-durable").exists(),
        "durable lease must not be written under the repo target tree"
    );
    let heads = cache_home.join("kiss").join("kiss-cov-durable").join("heads");
    assert!(
        heads.is_dir() && heads.read_dir().unwrap().next().is_some(),
        "publish must write a durable HEAD pointer for cheap cold hydrate"
    );

    fs::remove_dir_all(&kiss).unwrap();
    assert!(try_hydrate_if_kiss_absent(&repo, required, &[]));
    assert!(kiss.join("rslip_cache/hosts/x").is_file());
    assert!(kiss.join("cov_records_cache.json").is_file());
    assert!(
        !kiss.join("rust_llvm_cov_cache").exists(),
        "Python-only lease must not restore Rust cache"
    );
}

#[test]
fn hydrate_without_durable_generation_returns_false() {
    let tmp = tempdir().unwrap();
    let required = RequiredCoverageLanguages {
        python: true,
        rust: false,
    };
    assert!(!try_hydrate_if_kiss_absent(tmp.path(), required, &[]));
    assert!(!tmp.path().join(".kiss").exists());
}
