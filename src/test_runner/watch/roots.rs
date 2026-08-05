//! Resolve native watch registrations from a test invocation.

use std::path::{Path, PathBuf};

use crate::bin_cli::args::TestInvocation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchRootKind {
    Recursive,
    NonRecursive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WatchRegistration {
    pub path: PathBuf,
    pub kind: WatchRootKind,
}

pub(crate) fn resolve_watch_registrations(
    repo_root: &Path,
    invocation: &TestInvocation,
    _ignore: &[String],
) -> Result<Vec<WatchRegistration>, String> {
    let mut regs: Vec<WatchRegistration> = Vec::new();
    match invocation {
        TestInvocation::All | TestInvocation::Commit | TestInvocation::Base | TestInvocation::Main => {
            regs.push(WatchRegistration {
                path: repo_root.to_path_buf(),
                kind: WatchRootKind::Recursive,
            });
        }
        TestInvocation::Targets(targets) => {
            for raw in targets {
                push_target_registration(repo_root, raw, &mut regs)?;
            }
        }
    }
    push_support_registrations(repo_root, invocation, &mut regs);
    Ok(dedup_registrations(regs))
}

fn push_target_registration(
    repo_root: &Path,
    raw: &str,
    regs: &mut Vec<WatchRegistration>,
) -> Result<(), String> {
    let path_part = raw.split_once("::").map_or(raw, |(path, _)| path);
    let abs = resolve_target_abs(repo_root, path_part);
    if is_source_file_operand(path_part, &abs) {
        let parent = abs.parent().unwrap_or(repo_root).to_path_buf();
        regs.push(WatchRegistration {
            path: parent,
            kind: WatchRootKind::NonRecursive,
        });
        return Ok(());
    }
    regs.push(WatchRegistration {
        path: if abs.exists() {
            abs
        } else {
            repo_root.join(path_part)
        },
        kind: WatchRootKind::Recursive,
    });
    Ok(())
}

fn resolve_target_abs(repo_root: &Path, path_part: &str) -> PathBuf {
    let candidate = if Path::new(path_part).is_absolute() {
        PathBuf::from(path_part)
    } else {
        repo_root.join(path_part)
    };
    if let Ok(canon) = candidate.canonicalize() {
        return canon;
    }
    if let Some(parent) = candidate.parent()
        && let Ok(parent_canon) = parent.canonicalize()
        && let Some(name) = candidate.file_name()
    {
        return parent_canon.join(name);
    }
    candidate
}

fn is_source_file_operand(path_part: &str, abs: &Path) -> bool {
    abs.is_file()
        || Path::new(path_part)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("rs"))
}

fn push_support_registrations(
    repo_root: &Path,
    invocation: &TestInvocation,
    regs: &mut Vec<WatchRegistration>,
) {
    regs.push(WatchRegistration {
        path: repo_root.to_path_buf(),
        kind: WatchRootKind::NonRecursive,
    });
    push_python_ancestor_support_roots(repo_root, invocation, regs);
    if matches!(
        invocation,
        TestInvocation::Commit | TestInvocation::Base | TestInvocation::Main
    ) {
        let git = repo_root.join(".git");
        regs.push(WatchRegistration {
            path: git.clone(),
            kind: WatchRootKind::NonRecursive,
        });
        if matches!(invocation, TestInvocation::Base | TestInvocation::Main) {
            regs.push(WatchRegistration {
                path: git.join("refs").join("heads"),
                kind: WatchRootKind::NonRecursive,
            });
        }
    }
    regs.push(WatchRegistration {
        path: repo_root.join(".git").join("info"),
        kind: WatchRootKind::NonRecursive,
    });
}

fn push_python_ancestor_support_roots(
    repo_root: &Path,
    invocation: &TestInvocation,
    regs: &mut Vec<WatchRegistration>,
) {
    let TestInvocation::Targets(targets) = invocation else {
        return;
    };
    for raw in targets {
        let path_part = raw.split_once("::").map_or(raw.as_str(), |(path, _)| path);
        let abs = resolve_target_abs(repo_root, path_part);
        if !is_python_collection_root(path_part, &abs) {
            continue;
        }
        push_ancestors_to_repo_root(repo_root, &abs, regs);
    }
}

fn is_python_collection_root(path_part: &str, abs: &Path) -> bool {
    if Path::new(path_part)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
        || abs
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        return true;
    }
    if Path::new(path_part)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        || abs
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
    {
        return false;
    }
    abs.is_dir() || !is_source_file_operand(path_part, abs)
}

fn push_ancestors_to_repo_root(
    repo_root: &Path,
    start: &Path,
    regs: &mut Vec<WatchRegistration>,
) {
    let mut cur = if start.is_file()
        || start
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        start.parent().map(Path::to_path_buf)
    } else {
        Some(start.to_path_buf())
    };
    let repo_canon = repo_root.canonicalize().unwrap_or_else(|_| repo_root.to_path_buf());
    while let Some(dir) = cur {
        regs.push(WatchRegistration {
            path: dir.clone(),
            kind: WatchRootKind::NonRecursive,
        });
        let dir_canon = dir.canonicalize().unwrap_or_else(|_| dir.clone());
        if dir_canon == repo_canon || dir == repo_root {
            break;
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        if !parent.starts_with(repo_root) && !dir_canon.starts_with(&repo_canon) {
            break;
        }
        cur = Some(parent.to_path_buf());
    }
}

fn dedup_registrations(regs: Vec<WatchRegistration>) -> Vec<WatchRegistration> {
    let mut map = std::collections::BTreeMap::new();
    for reg in regs {
        map.entry(reg.path)
            .and_modify(|kind: &mut WatchRootKind| {
                if matches!(reg.kind, WatchRootKind::Recursive) {
                    *kind = WatchRootKind::Recursive;
                }
            })
            .or_insert(reg.kind);
    }
    map.into_iter()
        .map(|(path, kind)| WatchRegistration { path, kind })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_registers_repo_root_recursive() {
        let tmp = tempfile::tempdir().unwrap();
        let regs = resolve_watch_registrations(tmp.path(), &TestInvocation::All, &[]).unwrap();
        assert!(regs.iter().any(|r| {
            r.path == tmp.path() && matches!(r.kind, WatchRootKind::Recursive)
        }));
    }

    #[test]
    fn file_target_registers_parent_non_recursive() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("src");
        std::fs::create_dir_all(&file).unwrap();
        let py = file.join("a.py");
        std::fs::write(&py, "x=1\n").unwrap();
        let regs = resolve_watch_registrations(
            tmp.path(),
            &TestInvocation::Targets(vec!["src/a.py".into()]),
            &[],
        )
        .unwrap();
        assert!(regs.iter().any(|r| {
            path_eq_canon(&r.path, &file) && matches!(r.kind, WatchRootKind::NonRecursive)
        }));
    }

    #[test]
    fn nested_python_file_registers_ancestor_dirs_for_conftest() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("src").join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("a.py"), "x=1\n").unwrap();
        let regs = resolve_watch_registrations(
            tmp.path(),
            &TestInvocation::Targets(vec!["src/pkg/a.py".into()]),
            &[],
        )
        .unwrap();
        let src = tmp.path().join("src");
        assert!(
            regs.iter().any(|r| {
                path_eq_canon(&r.path, &src) && matches!(r.kind, WatchRootKind::NonRecursive)
            }),
            "expected non-recursive watch on ancestor src/ for conftest.py; regs={regs:?}"
        );
        assert!(regs.iter().any(|r| {
            path_eq_canon(&r.path, tmp.path()) && matches!(r.kind, WatchRootKind::NonRecursive)
        }));
    }

    fn path_eq_canon(left: &Path, right: &Path) -> bool {
        match (left.canonicalize(), right.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => left == right,
        }
    }
}
