use std::path::Path;

use super::resolve::resolve_target;
use super::resolved::{ResolvedTarget, SourceRegion};
use super::types::{TargetFocus, TargetRequest};

pub(crate) fn coverage_exit_from_ready_request(
    request: &TargetRequest,
    coverage_all: bool,
) -> Option<i32> {
    let cwd = std::env::current_dir().ok()?;
    let repo = crate::test_git::git_repo_root(&cwd).ok()?;
    let report = super::bind::load_ready_for_request(&repo, request, coverage_all, &[])?;
    for line in super::render::official_coverage_text(&report).lines() {
        crate::test_runner::emit_test_progress(line);
    }
    Some(report.exit_code)
}

pub(crate) fn coverage_paths_for_request(request: &TargetRequest) -> Result<Vec<String>, String> {
    let cwd = std::env::current_dir().map_err(|err| err.to_string())?;
    let repo = crate::test_git::git_repo_root(&cwd)?;
    focused_coverage_paths(&repo, request)
}

pub(crate) fn focused_coverage_paths(
    repo_root: &Path,
    request: &TargetRequest,
) -> Result<Vec<String>, String> {
    match &request.focus {
        TargetFocus::Workspace => Ok(vec![".".into()]),
        TargetFocus::Git(_) | TargetFocus::Operands(_) => {
            let resolved = resolve_target(repo_root, request)?;
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
                coverage_file_path(repo_root, path)
            }
        })
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

fn coverage_file_path(repo_root: &Path, raw: &str) -> String {
    let path_part = raw.split_once("::").map_or(raw, |(path, _)| path);
    if path_part.is_empty() || path_part == "." || path_part == "./" {
        return ".".into();
    }
    let path = Path::new(path_part);
    path.strip_prefix(repo_root)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path_part.to_string())
}

#[cfg(test)]
mod coverage_path_tests {
    use super::*;
    use crate::test_runner::target_request::{operands_request, workspace_request};
    use crate::test_runner::test_mode_fixtures::{git_in, init_git};
    use std::fs;

    fn workspace_req() -> TargetRequest {
        workspace_request(None, &[])
    }

    #[test]
    fn coverage_workspace_req_is_workspace_request() {
        assert_eq!(workspace_req(), workspace_request(None, &[]));
    }

    #[test]
    fn workspace_focus_is_dot() {
        let tmp = tempfile::TempDir::new().unwrap();
        init_git(&tmp);
        assert_eq!(
            focused_coverage_paths(tmp.path(), &workspace_req()).unwrap(),
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
        let paths = focused_coverage_paths(tmp.path(), &request).unwrap();
        assert_eq!(paths, vec!["src_foo.py".to_string()]);
    }

    #[test]
    fn nodeid_operand_uses_file_path() {
        assert_eq!(
            coverage_file_path(Path::new("/repo"), "test_lib.py::test_f"),
            "test_lib.py"
        );
        assert_eq!(
            coverage_file_path(Path::new("/repo"), "/repo/src_foo.py"),
            "src_foo.py"
        );
    }

    #[test]
    fn ready_coverage_exit_is_none_without_store() {
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        init_git(&tmp);
        let restore = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let exit = coverage_exit_from_ready_request(&workspace_req(), false);
        std::env::set_current_dir(restore).unwrap();
        assert_eq!(exit, None);
    }

    #[test]
    fn coverage_paths_for_request_uses_file_operand() {
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
        let paths = coverage_paths_for_request(&request);
        std::env::set_current_dir(restore).unwrap();
        assert_eq!(paths.unwrap(), vec!["src_foo.py".to_string()]);
    }
}
