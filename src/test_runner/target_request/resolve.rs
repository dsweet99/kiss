use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::test_git::TestChangeMode;
use kiss::Language;
use kiss::code_roles::is_python_test_module_path;

use super::history::historical_covering_selectors;
use super::projection::remember_target_plan;
use super::resolved::{OperandClass, ResolvedTarget, SourceRegion};
use super::stamp::capture_git_dep_stamp;
use super::types::{GitFocus, OperandExpr, TargetFocus, TargetRequest};

pub(crate) fn resolve_target(
    repo_root: &Path,
    request: &TargetRequest,
) -> Result<ResolvedTarget, String> {
    let resolved = resolve_only(repo_root, request)?;
    remember_target_plan(repo_root, request, &resolved);
    Ok(resolved)
}

pub(crate) fn resolve_only(
    repo_root: &Path,
    request: &TargetRequest,
) -> Result<ResolvedTarget, String> {
    match &request.focus {
        TargetFocus::Workspace => Ok(ResolvedTarget::workspace()),
        TargetFocus::Git(focus) => resolve_git(repo_root, request, focus),
        TargetFocus::Operands(operands) => resolve_operands(repo_root, request, operands),
    }
}

fn resolve_git(
    repo_root: &Path,
    request: &TargetRequest,
    focus: &GitFocus,
) -> Result<ResolvedTarget, String> {
    super::counters::add_git();
    let git_stamp = Some(capture_git_dep_stamp(repo_root, focus)?);
    let (mode, main_cfg, main_cli, base_cli) = git_resolve_args(focus);
    let diff_target =
        crate::test_git::resolve_diff_target(repo_root, mode, main_cfg, main_cli, base_cli)?;
    let rel_changed = match mode {
        TestChangeMode::Commit => crate::test_git::changed_paths_commit(repo_root)?,
        TestChangeMode::Base | TestChangeMode::Main => crate::test_git::changed_paths_since(
            repo_root,
            diff_target.as_ref().ok_or("missing git diff target")?,
        )?,
    };
    let rel_lines = match mode {
        TestChangeMode::Commit => crate::test_git::changed_lines_commit(repo_root)?,
        TestChangeMode::Base | TestChangeMode::Main => crate::test_git::changed_lines_since(
            repo_root,
            diff_target.as_ref().ok_or("missing git diff target")?,
        )?,
    };
    let lang = request.lang.map(super::types::LangFilter::to_language);
    Ok(git_resolved(
        repo_root,
        &rel_changed,
        &rel_lines,
        lang,
        git_stamp,
    ))
}

fn git_resolve_args(
    focus: &GitFocus,
) -> (TestChangeMode, Option<&str>, Option<&str>, Option<&str>) {
    match focus {
        GitFocus::Commit => (TestChangeMode::Commit, None, None, None),
        GitFocus::AutomaticBase => (TestChangeMode::Base, None, None, None),
        GitFocus::ExplicitBase { branch } => {
            (TestChangeMode::Base, None, None, Some(branch.as_str()))
        }
        GitFocus::DefaultMain => (TestChangeMode::Main, None, None, None),
        GitFocus::ConfiguredMain { name } => {
            (TestChangeMode::Main, Some(name.as_str()), None, None)
        }
        GitFocus::ExplicitMain { branch } => {
            (TestChangeMode::Main, None, Some(branch.as_str()), None)
        }
    }
}

fn git_resolved(
    repo_root: &Path,
    rel_changed: &[String],
    rel_lines: &std::collections::BTreeMap<String, BTreeSet<u32>>,
    lang: Option<Language>,
    git_stamp: Option<super::stamp::GitDepStamp>,
) -> ResolvedTarget {
    let mut regions = Vec::new();
    let mut historical_paths = Vec::new();
    for rel in rel_changed {
        let abs = repo_root.join(rel);
        if !abs.exists() {
            historical_paths.push(rel.clone());
            continue;
        }
        if !lang_allows(&abs, lang) {
            continue;
        }
        match rel_lines.get(rel) {
            Some(lines) if !lines.is_empty() => regions.push(SourceRegion::FileLines {
                path: rel.clone(),
                lines: lines.clone(),
            }),
            _ => regions.push(SourceRegion::FileAll { path: rel.clone() }),
        }
    }
    historical_paths.sort();
    let direct_selectors = historical_covering_selectors(repo_root, &historical_paths);
    ResolvedTarget {
        regions,
        direct_selectors,
        historical_paths,
        git_stamp,
        operand_classes: Vec::new(),
    }
}

fn resolve_operands(
    repo_root: &Path,
    request: &TargetRequest,
    operands: &[OperandExpr],
) -> Result<ResolvedTarget, String> {
    super::counters::add_parse();
    let lang = request.lang.map(super::types::LangFilter::to_language);
    let raws: Vec<String> = operands.iter().map(|operand| operand.raw.clone()).collect();
    let expanded = crate::test_runner::targets::expand_target_operands(
        repo_root,
        &raws,
        &request.ignore,
        lang,
    )?;
    match expanded {
        crate::test_runner::targets::ExpandedTargetPlan::All => Ok(ResolvedTarget::workspace()),
        crate::test_runner::targets::ExpandedTargetPlan::Files(files) => {
            classify_expanded(repo_root, request, operands, files, lang)
        }
    }
}

fn classify_expanded(
    repo_root: &Path,
    request: &TargetRequest,
    operands: &[OperandExpr],
    files: Vec<String>,
    lang: Option<Language>,
) -> Result<ResolvedTarget, String> {
    let query = crate::test_runner::targets::resolve_target_operands(
        repo_root,
        &files,
        lang,
        &request.ignore,
        &[],
    )?;
    let mut regions = regions_from_query(&query);
    let mut direct_selectors: Vec<String> = query
        .direct_python
        .into_iter()
        .chain(query.direct_rust)
        .collect();
    direct_selectors.sort();
    if regions.is_empty() && !files.is_empty() {
        regions = files
            .iter()
            .filter(|path| is_production_source(repo_root, path))
            .map(|path| SourceRegion::FileAll {
                path: repo_rel(repo_root, path),
            })
            .collect();
    }
    Ok(ResolvedTarget {
        regions,
        direct_selectors,
        historical_paths: Vec::new(),
        git_stamp: None,
        operand_classes: operands
            .iter()
            .map(|operand| classify_operand(repo_root, &operand.raw))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn regions_from_query(
    query: &crate::test_runner::targets::TargetSelectionQuery,
) -> Vec<SourceRegion> {
    let mut regions = Vec::new();
    for (path, lines) in query.python_lines.iter().chain(query.rust_lines.iter()) {
        regions.push(SourceRegion::FileLines {
            path: path.to_string_lossy().replace('\\', "/"),
            lines: lines.clone(),
        });
    }
    for path in query.python_files.iter().chain(query.rust_files.iter()) {
        regions.push(SourceRegion::FileAll {
            path: path.to_string_lossy().replace('\\', "/"),
        });
    }
    regions
}

fn classify_operand(repo_root: &Path, raw: &str) -> Result<OperandClass, String> {
    let path_part = raw.split_once("::").map_or(raw, |(path, _)| path);
    let candidate = if Path::new(path_part).is_absolute() {
        PathBuf::from(path_part)
    } else {
        repo_root.join(path_part)
    };
    let abs = candidate
        .canonicalize()
        .map_err(|_| format!("target '{raw}': path not found at {}", candidate.display()))?;
    if abs.is_dir() {
        return Ok(OperandClass::Directory);
    }
    if !abs.is_file() {
        return Err(format!("target '{raw}': path is not a file or directory"));
    }
    Ok(classify_file_operand(&abs, raw))
}

fn classify_file_operand(abs: &Path, raw: &str) -> OperandClass {
    let symbol = raw.contains("::");
    let nodeid = raw.contains('[') || raw.matches("::").count() > 1;
    let test_file = is_python_test_module_path(abs) || path_under_tests(abs);
    match (nodeid, test_file, symbol) {
        (true, _, _) => OperandClass::PythonNodeid,
        (false, true, true) => OperandClass::TestSymbol,
        (false, true, false) => OperandClass::TestFile,
        (false, false, true) => OperandClass::SourceSymbol,
        (false, false, false) => OperandClass::SourceFile,
    }
}

fn path_under_tests(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "tests")
}

fn lang_allows(path: &Path, lang: Option<Language>) -> bool {
    match lang {
        None => Language::from_path(path).is_some(),
        Some(filter) => Language::from_path(path) == Some(filter),
    }
}

fn is_production_source(repo_root: &Path, path: &str) -> bool {
    let path_part = path.split_once("::").map_or(path, |(file, _)| file);
    if path_part.is_empty() {
        return false;
    }
    let abs = if Path::new(path_part).is_absolute() {
        PathBuf::from(path_part)
    } else {
        repo_root.join(path_part)
    };
    abs.is_file() && !is_python_test_module_path(&abs)
}

fn repo_rel(repo_root: &Path, path: &str) -> String {
    let path_part = path.split_once("::").map_or(path, |(file, _)| file);
    let abs = Path::new(path_part);
    abs.strip_prefix(repo_root)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path_part.replace('\\', "/"))
}
