use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{TestChangeMode, TestLangFilter};

pub fn changed_paths_commit(repo: &Path) -> Result<Vec<String>, String> {
    let mut names = BTreeSet::new();
    names.extend(changed_paths_from_diff(repo, &["diff"], Some("HEAD"))?);
    let u = super::git_output(repo, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    names.extend(super::diff_paths::nul_paths(&u));
    Ok(names.into_iter().collect())
}

pub fn changed_paths_since(repo: &Path, rev: &str) -> Result<Vec<String>, String> {
    changed_paths_from_diff(repo, &["diff"], Some(rev))
}

fn changed_paths_from_diff(
    repo: &Path,
    diff_prefix: &[&str],
    rev: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut names = BTreeSet::new();
    for filter in ["AM", "D"] {
        let mut args: Vec<&str> = diff_prefix.to_vec();
        args.extend(["--no-renames", "-z", "--name-only", "--diff-filter", filter]);
        if let Some(rev) = rev {
            args.push(rev);
        }
        let out = super::git_output(repo, &args)?;
        names.extend(super::diff_paths::nul_paths(&out));
    }
    Ok(names.into_iter().collect())
}

pub fn changed_lines_commit(repo: &Path) -> Result<BTreeMap<String, BTreeSet<u32>>, String> {
    changed_lines_for_diff(repo, "HEAD")
}

pub fn changed_lines_since(
    repo: &Path,
    rev: &str,
) -> Result<BTreeMap<String, BTreeSet<u32>>, String> {
    changed_lines_for_diff(repo, rev)
}

fn changed_lines_for_diff(
    repo: &Path,
    rev: &str,
) -> Result<BTreeMap<String, BTreeSet<u32>>, String> {
    let diff = super::git_output(
        repo,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--no-renames",
            "--unified=0",
            "--diff-filter=AM",
            rev,
        ],
    )?;
    Ok(parse_changed_lines_from_unified_diff(&diff))
}

pub(crate) fn parse_changed_lines_from_unified_diff(diff: &str) -> BTreeMap<String, BTreeSet<u32>> {
    let mut out = BTreeMap::new();
    let mut current_file: Option<String> = None;
    let mut accept_plus_header = true;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            current_file = None;
            accept_plus_header = true;
            continue;
        }
        if accept_plus_header && let Some(path) = line.strip_prefix("+++ ") {
            current_file = super::diff_paths::plus_line_path(path);
            continue;
        }
        if !line.starts_with("@@") {
            continue;
        }
        accept_plus_header = false;
        let Some(file) = current_file.as_ref() else {
            continue;
        };
        let Some(range) = line.split_whitespace().nth(2) else {
            continue;
        };
        let Some((start, len)) = parse_unified_new_range(range) else {
            continue;
        };
        if len == 0 {
            continue;
        }
        let lines = out.entry(file.clone()).or_insert_with(BTreeSet::new);
        for line_no in start..start.saturating_add(len) {
            lines.insert(line_no);
        }
    }
    out
}

fn parse_unified_new_range(range: &str) -> Option<(u32, u32)> {
    let range = range.strip_prefix('+')?;
    let (start, len) = range
        .split_once(',')
        .map_or((range, "1"), |(start, len)| (start, len));
    Some((start.parse().ok()?, len.parse().ok()?))
}

fn rel_path_ignored(rel: &str, ignore: &[String]) -> bool {
    kiss::path_ignored_by_prefixes(rel, ignore)
}

fn lang_ok(path: &Path, lang_filter: Option<TestLangFilter>) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match lang_filter {
        None => ext.eq_ignore_ascii_case("py") || is_rust_planning_path(path),
        Some(TestLangFilter::Python) => ext.eq_ignore_ascii_case("py"),
        Some(TestLangFilter::Rust) => is_rust_planning_path(path),
    }
}

pub(crate) fn is_rust_planning_path(path: &Path) -> bool {
    if kiss::Language::is_rust_path(path)
        || path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("inc"))
    {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let in_cargo_dir = path
        .parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|dir| dir == ".cargo");
    matches!(name, "Cargo.toml" | "Cargo.lock" | "config.toml")
        || (in_cargo_dir && name == "config")
        || name.starts_with("rust-toolchain")
}

pub fn resolve_changed_source_paths(
    repo_root: &Path,
    rel_names: &[String],
    ignore: &[String],
    lang_filter: Option<TestLangFilter>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for rel in rel_names {
        if rel_path_ignored(rel, ignore) {
            continue;
        }
        let abs = repo_root.join(rel);
        let include_missing = lang_ok(&abs, lang_filter);
        match abs.metadata() {
            Ok(meta) if meta.is_file() => {
                if !lang_ok(&abs, lang_filter) {
                    continue;
                }
                if let Ok(c) = abs.canonicalize() {
                    out.push(c);
                }
            }
            _ if include_missing => {
                out.push(abs);
            }
            _ => continue,
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn resolve_diff_target(
    repo: &Path,
    mode: TestChangeMode,
    main_branch_cfg: Option<&str>,
    main_branch_cli: Option<&str>,
    base_branch_cli: Option<&str>,
) -> Result<Option<String>, String> {
    match mode {
        TestChangeMode::Commit => Ok(None),
        TestChangeMode::Main => {
            let m = super::resolve_main_branch_name(repo, main_branch_cfg, main_branch_cli)?;
            Ok(Some(m))
        }
        TestChangeMode::Base => base_branch_cli.map_or_else(
            || super::auto_detect_fork_commit(repo).map(Some),
            |b| super::merge_base(repo, b).map(Some),
        ),
    }
}
