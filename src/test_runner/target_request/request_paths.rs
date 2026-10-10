use std::path::Path;

use super::resolve::resolve_only;
use super::resolved::{ResolvedTarget, SourceRegion};
use super::types::{TargetFocus, TargetRequest};

pub(crate) fn request_source_paths(request: &TargetRequest) -> Result<Vec<String>, String> {
    let cwd = std::env::current_dir().map_err(|err| err.to_string())?;
    let repo = crate::test_git::git_repo_root(&cwd)?;
    focused_source_paths(&repo, request)
}

pub(crate) fn focused_source_paths(
    repo_root: &Path,
    request: &TargetRequest,
) -> Result<Vec<String>, String> {
    match &request.focus {
        TargetFocus::Workspace => Ok(vec![".".into()]),
        TargetFocus::Git(_) | TargetFocus::Operands(_) => {
            let resolved = resolve_only(repo_root, request)?;
            Ok(region_paths(repo_root, &resolved))
        }
    }
}

fn region_paths(repo_root: &Path, resolved: &ResolvedTarget) -> Vec<String> {
    let mut paths: Vec<String> = resolved
        .regions
        .iter()
        .map(|region| match region {
            SourceRegion::WorkspaceAll => ".".into(),
            SourceRegion::FileAll { path } | SourceRegion::FileLines { path, .. } => {
                request_file_path(repo_root, path)
            }
        })
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

fn request_file_path(repo_root: &Path, raw: &str) -> String {
    let (path_part, _) = kiss::split_selector(raw);
    if path_part.is_empty() || path_part == "." || path_part == "./" {
        return ".".into();
    }
    let path = Path::new(path_part);
    path.strip_prefix(repo_root)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path_part.to_string())
}

#[cfg(test)]
mod request_path_tests {
    use super::*;
    use crate::test_runner::target_request::{operands_request, workspace_request};
    use crate::test_runner::test_mode_fixtures::{git_in, init_git};
    use std::fs;

    fn workspace_req() -> TargetRequest {
        workspace_request(None, &[])
    }

    #[test]
    fn workspace_req_is_workspace_request() {
        assert_eq!(workspace_req(), workspace_request(None, &[]));
    }

    #[test]
    fn workspace_focus_is_dot() {
        let tmp = tempfile::TempDir::new().unwrap();
        init_git(&tmp);
        assert_eq!(
            focused_source_paths(tmp.path(), &workspace_req()).unwrap(),
            vec![".".to_string()]
        );
    }

    #[test]
    fn file_operand_uses_resolved_region() {
        let tmp = tempfile::TempDir::new().unwrap();
        init_git(&tmp);
        fs::write(tmp.path().join("src_foo.py"), "x = 1\n").unwrap();
        assert!(
            git_in(tmp.path())
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            git_in(tmp.path())
                .args(["commit", "-m", "seed"])
                .status()
                .unwrap()
                .success()
        );
        let request = operands_request(&["src_foo.py".into()], None, &[]);
        let paths = focused_source_paths(tmp.path(), &request).unwrap();
        assert_eq!(paths, vec!["src_foo.py".to_string()]);
    }

    #[test]
    fn nodeid_operand_uses_file_path() {
        assert_eq!(
            request_file_path(Path::new("/repo"), "test_lib.py::test_f"),
            "test_lib.py"
        );
        assert_eq!(
            request_file_path(Path::new("/repo"), "/repo/src_foo.py"),
            "src_foo.py"
        );
    }

    #[test]
    fn request_source_paths_uses_file_operand() {
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        init_git(&tmp);
        fs::write(tmp.path().join("src_foo.py"), "x = 1\n").unwrap();
        assert!(
            git_in(tmp.path())
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            git_in(tmp.path())
                .args(["commit", "-m", "seed"])
                .status()
                .unwrap()
                .success()
        );
        let request = operands_request(&["src_foo.py".into()], None, &[]);
        let restore = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let paths = request_source_paths(&request);
        std::env::set_current_dir(restore).unwrap();
        assert_eq!(paths.unwrap(), vec!["src_foo.py".to_string()]);
    }
}
