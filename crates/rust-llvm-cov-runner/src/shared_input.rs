use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::RustLlvmCovError;
use crate::batch_plan::RustCoverageBatchRequest;
use crate::cargo_workspace_metadata::workspace_metadata_from_cargo;

pub fn workspace_input_digest(root: &Path) -> io::Result<String> {
    digest_input_files(root, &rust_cov_input_files(root)?)
}

fn selection_context_input_files(
    root: &Path,
    req: &RustCoverageBatchRequest,
) -> Result<Vec<PathBuf>, RustLlvmCovError> {
    let files = rust_cov_input_files(root)?;
    let metadata = workspace_metadata_from_cargo(&req.cwd, &req.cargo, &req.cargo_args).ok();
    let mut selected = Vec::new();
    for file in files {
        if file
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        {
            let include = match metadata.as_ref() {
                Some(metadata) => metadata
                    .rs_compile_time_classification(root, &file)
                    .unwrap_or(true),
                None => true,
            };
            if include {
                selected.push(file);
            }
        } else {
            selected.push(file);
        }
    }
    Ok(selected)
}

pub fn selection_context_source_digest(
    root: &Path,
    req: &RustCoverageBatchRequest,
) -> Result<String, RustLlvmCovError> {
    let selected = selection_context_input_files(root, req)?;
    digest_input_files(root, &selected).map_err(RustLlvmCovError::Io)
}

fn digest_input_files(root: &Path, files: &[PathBuf]) -> io::Result<String> {
    let _ = root;
    let mut h = super::rust_cov_cache::rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"shared-input-v1");
    for file in files {
        h = super::rust_cov_cache::rust_cov_fnv1a64(h, file.to_string_lossy().as_bytes());
        h = super::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
        h = super::rust_cov_cache::rust_cov_fnv1a64(h, &fs::read(file)?);
        h = super::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

pub fn rust_cov_input_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_rust_cov_inputs(root, &mut out)?;
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    Ok(out)
}

fn visit_rust_cov_inputs(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_skip_rust_cov_dir(&path) {
                continue;
            }
            visit_rust_cov_inputs(&path, out)?;
        } else if file_type.is_file() && is_rust_cov_cache_input(&path) {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn should_skip_rust_cov_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".git" | "target" | ".rust_llvm_cov_cache")
    ) || is_kiss_rust_cov_cache_dir(path)
}

pub(crate) fn is_kiss_rust_cov_cache_dir(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("rust_llvm_cov_cache")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some(".kiss")
}

pub(crate) fn is_rust_cov_cache_input(path: &Path) -> bool {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
    {
        return true;
    }
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Cargo.toml" | "Cargo.lock" | "config.toml")
    ) || is_cargo_config_input_path(path)
        || is_rust_toolchain_input_path(path)
}

pub fn is_cargo_config_input_path(path: &Path) -> bool {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        == Some(".cargo")
        && matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("config" | "config.toml")
        )
}

pub(crate) fn is_rust_toolchain_input_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("rust-toolchain"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn selection_context_input_file_set_is_stable_across_ordinary_lib_rs_edits() {
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
        let _ = crate::cargo_workspace_metadata::workspace_metadata_from_cargo(
            &req.cwd,
            &req.cargo,
            &req.cargo_args,
        );
        let before = selection_context_input_files(tmp.path(), &req).unwrap();
        fs::write(tmp.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
        let after = selection_context_input_files(tmp.path(), &req).unwrap();
        assert_eq!(before, after);
        assert!(!before.iter().any(|path| path.ends_with("lib.rs")));
    }

    #[test]
    fn selection_context_input_files_exclude_ordinary_lib_rs() {
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
        let metadata =
            crate::cargo_workspace_metadata::workspace_metadata_from_cargo(&req.cwd, &req.cargo, &req.cargo_args)
                .expect("metadata");
        let files = rust_cov_input_files(tmp.path()).unwrap();
        let lib = files
            .iter()
            .find(|path| path.ends_with("lib.rs"))
            .expect("lib.rs");
        assert_eq!(
            metadata.rs_compile_time_classification(tmp.path(), lib),
            Some(false)
        );
    }

    #[test]
    fn selection_context_source_digest_ignores_ordinary_lib_rs_edits() {
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
        let _ = crate::cargo_workspace_metadata::workspace_metadata_from_cargo(
            &req.cwd,
            &req.cargo,
            &req.cargo_args,
        );
        let before = selection_context_source_digest(tmp.path(), &req).unwrap();
        fs::write(tmp.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
        let after = selection_context_source_digest(tmp.path(), &req).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn workspace_input_digest_scans_once_and_is_stable() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

        let first = workspace_input_digest(tmp.path()).unwrap();
        let second = workspace_input_digest(tmp.path()).unwrap();
        assert_eq!(first, second);

        fs::write(tmp.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
        let changed = workspace_input_digest(tmp.path()).unwrap();
        assert_ne!(first, changed);
    }

    #[test]
    fn input_files_skip_target_and_cache_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("target")).unwrap();
        fs::create_dir_all(tmp.path().join(".kiss").join("rust_llvm_cov_cache")).unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(tmp.path().join("target").join("ignored.rs"), "x\n").unwrap();

        let names: BTreeSet<_> = rust_cov_input_files(tmp.path())
            .unwrap()
            .into_iter()
            .map(|path| path.strip_prefix(tmp.path()).unwrap().to_path_buf())
            .collect();
        assert!(names.contains(Path::new("Cargo.toml")));
        assert!(!names.contains(Path::new("target/ignored.rs")));
    }

    #[test]
    fn input_scan_includes_cargo_config_and_toolchain_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".cargo")).unwrap();
        fs::create_dir_all(tmp.path().join("src").join("nested")).unwrap();
        fs::write(tmp.path().join(".cargo").join("config.toml"), "").unwrap();
        fs::write(tmp.path().join("rust-toolchain.toml"), "[toolchain]\n").unwrap();
        fs::write(tmp.path().join("Cargo.lock"), "").unwrap();
        fs::write(tmp.path().join("src").join("lib.rs"), "x\n").unwrap();
        fs::write(tmp.path().join("src").join("nested").join("mod.rs"), "y\n").unwrap();

        assert!(is_cargo_config_input_path(
            &tmp.path().join(".cargo").join("config.toml")
        ));
        assert!(is_cargo_config_input_path(
            &tmp.path().join(".cargo").join("config")
        ));
        assert!(!is_cargo_config_input_path(
            &tmp.path().join(".cargo").join("credentials")
        ));
        assert!(is_rust_toolchain_input_path(
            &tmp.path().join("rust-toolchain.toml")
        ));
        assert!(is_rust_cov_cache_input(
            &tmp.path().join("src").join("lib.rs")
        ));
        let config_path = tmp.path().join(".cargo").join("config.toml");
        let bare_config_path = tmp.path().join(".cargo").join("config");
        let recognizes_toml = is_cargo_config_input_path(&config_path);
        let recognizes_bare = is_cargo_config_input_path(&bare_config_path);
        let rejects_credentials =
            is_cargo_config_input_path(&tmp.path().join(".cargo").join("credentials"));
        assert!(recognizes_toml);
        assert!(recognizes_bare);
        assert!(!rejects_credentials);
        let names: BTreeSet<_> = rust_cov_input_files(tmp.path())
            .unwrap()
            .into_iter()
            .map(|path| path.strip_prefix(tmp.path()).unwrap().to_path_buf())
            .collect();
        assert!(names.contains(Path::new(".cargo/config.toml")));
        assert!(names.contains(Path::new("rust-toolchain.toml")));
        assert!(names.contains(Path::new("Cargo.lock")));
        assert!(names.contains(Path::new("src/lib.rs")));
        assert!(names.contains(Path::new("src/nested/mod.rs")));
    }

    #[test]
    fn skip_helpers_recognize_kiss_cache_and_git_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let kiss_cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
        assert!(is_kiss_rust_cov_cache_dir(&kiss_cache));
        assert!(should_skip_rust_cov_dir(&tmp.path().join(".git")));
        assert!(should_skip_rust_cov_dir(&tmp.path().join("target")));
        assert!(should_skip_rust_cov_dir(
            &tmp.path().join(".rust_llvm_cov_cache")
        ));
        assert!(!should_skip_rust_cov_dir(&tmp.path().join("src")));
    }

    #[test]
    fn visit_rust_cov_inputs_collects_nested_sources_and_root_config() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src/deep")).unwrap();
        fs::write(tmp.path().join("config.toml"), "[package]\n").unwrap();
        fs::write(tmp.path().join("src/deep/mod.rs"), "pub fn z() {}\n").unwrap();

        let mut collected = Vec::new();
        visit_rust_cov_inputs(tmp.path(), &mut collected).unwrap();
        collected.sort();

        assert!(collected.iter().any(|path| path.ends_with("config.toml")));
        assert!(
            collected
                .iter()
                .any(|path| path.ends_with("src/deep/mod.rs"))
        );
        assert_eq!(rust_cov_input_files(tmp.path()).unwrap(), collected);
    }

    #[test]
    fn unclassifiable_repository_rs_is_included_in_selection_context_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("crate").join("src")).unwrap();
        fs::create_dir_all(tmp.path().join("misc")).unwrap();
        fs::write(
            tmp.path().join("crate").join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
        )
        .unwrap();
        fs::write(
            tmp.path().join("crate").join("src").join("lib.rs"),
            "pub fn x() {}\n",
        )
        .unwrap();
        fs::write(tmp.path().join("misc").join("orphan.rs"), "pub fn orphan() {}\n").unwrap();
        let mut req = crate::batch_plan::RustCoverageBatchRequest::witness();
        req.cwd = tmp.path().join("crate");
        req.source_root = tmp.path().to_path_buf();
        req.cargo_args.clear();
        let metadata = crate::cargo_workspace_metadata::workspace_metadata_from_cargo(
            &req.cwd,
            &req.cargo,
            &req.cargo_args,
        )
        .expect("metadata");
        let orphan = tmp.path().join("misc").join("orphan.rs");
        assert_eq!(
            metadata.rs_compile_time_classification(tmp.path(), &orphan),
            None
        );
        let files = selection_context_input_files(tmp.path(), &req).unwrap();
        assert!(files.iter().any(|path| path.ends_with("misc/orphan.rs")));
        assert!(!files.iter().any(|path| path.ends_with("crate/src/lib.rs")));
    }

    #[test]
    fn is_cargo_config_input_path_rejects_non_config_files() {
        let rejects_lib = is_cargo_config_input_path(Path::new("src/lib.rs"));
        let rejects_credentials = is_cargo_config_input_path(Path::new(".cargo/credentials"));
        let rejects_root_config = is_cargo_config_input_path(Path::new("config.toml"));
        assert!(!rejects_lib);
        assert!(!rejects_credentials);
        assert!(!rejects_root_config);
    }
}
