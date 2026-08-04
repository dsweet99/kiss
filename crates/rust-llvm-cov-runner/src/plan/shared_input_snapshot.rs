use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::RustLlvmCovError;
use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::plan::cargo_workspace_metadata::workspace_metadata_from_cargo;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RustInputSnapshot {
    pub(crate) input_digest: String,
    pub(crate) selection_context_source_digest: String,
    pub(crate) ordinary_source_digests: BTreeMap<String, String>,
}

pub(crate) fn rust_input_snapshot(
    root: &Path,
    req: &RustCoverageBatchRequest,
) -> Result<RustInputSnapshot, RustLlvmCovError> {
    let metadata = workspace_metadata_from_cargo(&req.cwd, &req.cargo, &req.cargo_args).ok();
    let files = super::rust_cov_input_files(root).map_err(RustLlvmCovError::Io)?;
    digest_input_file_snapshot(
        root,
        &files,
        |path| fs::read(path),
        |file| {
            if !super::is_rust_cov_cache_input(file)
                || !file.extension().is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("rs") || ext.eq_ignore_ascii_case("inc")
                })
            {
                return Ok(false);
            }
            let is_inc = file
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("inc"));
            let Some(metadata) = metadata.as_ref() else {
                return Ok(is_inc || is_default_ordinary_rust_source(file));
            };
            match metadata.rs_compile_time_classification(root, file) {
                Some(false) => Ok(true),
                Some(true) => Ok(false),
                None => Ok(is_inc || is_default_ordinary_rust_source(file)),
            }
        },
    )
}

fn is_default_ordinary_rust_source(file: &Path) -> bool {
    file.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        && file.file_name().and_then(|name| name.to_str()) != Some("build.rs")
}

pub(crate) fn digest_input_file_snapshot(
    root: &Path,
    files: &[PathBuf],
    mut read_file: impl FnMut(&Path) -> io::Result<Vec<u8>>,
    mut classify: impl FnMut(&Path) -> Result<bool, RustLlvmCovError>,
) -> Result<RustInputSnapshot, RustLlvmCovError> {
    let mut input_hash =
        crate::rust_cov_cache::rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"shared-input-v1");
    let mut selection_hash =
        crate::rust_cov_cache::rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"shared-input-v1");
    let mut ordinary_source_digests = BTreeMap::new();
    for file in files {
        let bytes = read_file(file).map_err(RustLlvmCovError::Io)?;
        input_hash = hash_input_file(input_hash, file, &bytes);
        if !classify(file)? {
            selection_hash = hash_input_file(selection_hash, file, &bytes);
        } else {
            let rel = crate::rust_cov_cache::repo_relative_path(root, file).ok_or_else(|| {
                RustLlvmCovError::InvalidRequest(format!(
                    "ordinary Rust source path is not repository-relative: {}",
                    file.display()
                ))
            })?;
            if !is_ordinary_source_rel_path(&rel) || rel.is_empty() {
                return Err(RustLlvmCovError::InvalidRequest(format!(
                    "ordinary Rust source path is not a repository-relative Rust path: {rel}"
                )));
            }
            if ordinary_source_digests
                .insert(rel.clone(), ordinary_source_content_digest(&bytes))
                .is_some()
            {
                return Err(RustLlvmCovError::InvalidRequest(format!(
                    "duplicate ordinary Rust source path: {rel}"
                )));
            }
        }
    }
    Ok(RustInputSnapshot {
        input_digest: format!("{input_hash:016x}"),
        selection_context_source_digest: format!("{selection_hash:016x}"),
        ordinary_source_digests,
    })
}

fn hash_input_file(mut h: u64, file: &Path, bytes: &[u8]) -> u64 {
    h = crate::rust_cov_cache::rust_cov_fnv1a64(h, file.to_string_lossy().as_bytes());
    h = crate::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    h = crate::rust_cov_cache::rust_cov_fnv1a64(h, bytes);
    crate::rust_cov_cache::rust_cov_fnv1a64(h, &[0])
}

fn ordinary_source_content_digest(bytes: &[u8]) -> String {
    let h = crate::rust_cov_cache::rust_cov_fnv1a64(
        0xcbf2_9ce4_8422_2325,
        b"ordinary-source-content-v1",
    );
    format!("{:016x}", crate::rust_cov_cache::rust_cov_fnv1a64(h, bytes))
}

fn is_ordinary_source_rel_path(path: &str) -> bool {
    path.ends_with(".rs") || path.ends_with(".inc")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_snapshot_accepts_selection_context_only_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = tmp.path().join("Cargo.toml");
        fs::write(&manifest, "[package]\n").unwrap();

        let snapshot = digest_input_file_snapshot(
            tmp.path(),
            std::slice::from_ref(&manifest),
            |path| fs::read(path),
            |_| Ok(false),
        )
        .unwrap();

        assert!(snapshot.ordinary_source_digests.is_empty());
        assert!(!snapshot.input_digest.is_empty());
        assert!(!snapshot.selection_context_source_digest.is_empty());
    }

    #[test]
    fn digest_snapshot_rejects_ordinary_non_rs_repo_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = tmp.path().join("Cargo.toml");
        fs::write(&manifest, "[package]\n").unwrap();

        let err = digest_input_file_snapshot(
            tmp.path(),
            &[manifest],
            |path| fs::read(path),
            |_| Ok(true),
        )
        .unwrap_err();

        assert!(format!("{err:?}").contains("repository-relative Rust path"));
    }

    #[test]
    fn ordinary_source_content_digest_is_content_only() {
        assert_eq!(
            ordinary_source_content_digest(b"same"),
            ordinary_source_content_digest(b"same")
        );
        assert_ne!(
            ordinary_source_content_digest(b"same"),
            ordinary_source_content_digest(b"different")
        );
    }
}
