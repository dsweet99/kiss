use std::path::{Path, PathBuf};

use crate::test_runner::lang_rust::workspace::{
    cargo_workspace_member_manifest_dirs, is_workspace_rust_selector_file,
};

pub(crate) fn filter_root_workspace_rust_cov_files(
    repo_root: &Path,
    rs_files: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let Ok(member_dirs) = cargo_workspace_member_manifest_dirs(repo_root) else {
        return rs_files;
    };
    rs_files
        .into_iter()
        .filter(|path| is_workspace_rust_selector_file(path, &member_dirs))
        .collect()
}
