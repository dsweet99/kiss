use std::path::Path;

use kiss::test_records::TestRecord;

pub(crate) use crate::test_runner::lang_iface::records::{
    Outcome, cache_policy, current_deps, declared_inputs_digest,
};
use crate::test_runner::lang_iface::records::{RecordScope, digest};

const IDENTITY_SCHEMA: &str = "kiss-rust-nextest-record-v1";

fn identity_args(extras: &[String]) -> Vec<&str> {
    extras
        .iter()
        .map(String::as_str)
        .filter(|arg| !matches!(*arg, "--nocapture" | "--no-capture"))
        .collect()
}

pub(crate) fn record_identity(repo_root: &Path, extras: &[String]) -> Result<String, String> {
    let toolchain = super::toolchain::current_rust_toolchain(repo_root)?;
    let payload = serde_json::json!({
        "schema": IDENTITY_SCHEMA,
        "toolchain": toolchain,
        "args": identity_args(extras),
        "env": super::env::identity_env(),
    });
    Ok(format!(
        "rust-nextest:{}",
        digest(payload.to_string().as_bytes())
    ))
}

pub(crate) fn rust_inputs_digest(repo_root: &Path) -> Result<String, String> {
    let sources =
        crate::test_runner::workspace_selector_cache::rust_full_source_fingerprint(repo_root, &[])
            .map_err(|err| format!("error: kiss: fingerprint Rust sources: {err}"))?;
    let included = crate::test_runner::lang_rust::rust_expanded_include_extras_fingerprint(
        repo_root,
        |parts| digest(&parts.concat()),
    );
    Ok(digest(format!("{sources}\0{included}").as_bytes()))
}

fn scope<'a>(repo_root: &'a Path, identity: &'a str) -> RecordScope<'a> {
    RecordScope {
        repo_root,
        language: "rust",
        identity,
    }
}

pub(crate) fn store(
    repo_root: &Path,
    identity: &str,
    inputs: &str,
    outcome: &Outcome<'_>,
) -> Result<(), String> {
    crate::test_runner::lang_iface::records::store(scope(repo_root, identity), inputs, outcome)
}

pub(crate) fn records_under(repo_root: &Path, identity: &str) -> Vec<TestRecord> {
    crate::test_runner::lang_iface::records::records_under(scope(repo_root, identity))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nocapture_does_not_change_identity_args() {
        let extras = vec!["--nocapture".to_string(), "--ignored".to_string()];
        assert_eq!(identity_args(&extras), ["--ignored"]);
    }

    #[test]
    fn inputs_digest_changes_with_rust_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let before = rust_inputs_digest(root).unwrap();
        assert_eq!(before, rust_inputs_digest(root).unwrap());
        std::fs::write(root.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
        assert_ne!(before, rust_inputs_digest(root).unwrap());
    }

    #[test]
    fn inputs_digest_sees_a_lockfile_written_mid_session_once_forgotten() {
        use crate::test_runner::workspace_selector_cache::{
            begin_inventory_session, forget_inventory,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let _session = begin_inventory_session(root);
        let before = rust_inputs_digest(root).unwrap();
        std::fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
        assert_eq!(before, rust_inputs_digest(root).unwrap());
        forget_inventory(root);
        assert_ne!(before, rust_inputs_digest(root).unwrap());
    }
}
