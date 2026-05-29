/// Class defs on optimizer paths: header/docstring lines only (constructor witness must not
/// credit entire class bodies vs slipcover).
#[allow(dead_code)]
pub(crate) const OPTIMIZER_CLASS_HEADER_LINES: usize = 3;

/// Inflator-path function defs: call witnesses usually reach headers only at runtime.
pub(crate) const INFLATOR_FUNCTION_HEADER_LINES: usize = 3;

/// `PKG/base/…` but not under `base/oi/`.
pub fn is_py_base_non_oi_subtree(path: &std::path::Path) -> bool {
    is_py_base_subtree_only(path) && !is_py_base_oi_subtree(path)
}

/// `PKG/base/oi/…/interfaces.py` — protocol stubs; runtime loads full module bodies on import.
pub fn is_py_oi_interfaces_stub_path(path: &std::path::Path) -> bool {
    is_py_base_oi_subtree(path)
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "interfaces.py")
}

/// Direct children of `base/oi/` (e.g. `objectdb.py`): parent-package imports must not
/// credit siblings; nested packages keep prefix binding.
pub fn is_py_oi_root_level_module(path: &std::path::Path) -> bool {
    if !is_py_base_oi_subtree(path) {
        return false;
    }
    let comps = path_normal_components(path);
    let oi_idx = comps.iter().position(|&c| c == "oi");
    let Some(oi_idx) = oi_idx else {
        return false;
    };
    comps.len() == oi_idx + 2
        && path
            .file_name()
            .and_then(|n| n.to_str())
            != Some("__init__.py")
}

pub fn calibration_def_end_line(def: &CodeDefinition) -> usize {
    // Protocol stubs: header-only credit; full bodies stay in denominator via split in coverage_map.
    if is_py_oi_interfaces_stub_path(&def.file) {
        return def
            .line
            .saturating_add(INFLATOR_FUNCTION_HEADER_LINES.saturating_sub(1))
            .min(def.end_line);
    }
    if def.kind == crate::units::CodeUnitKind::Class
        && (is_py_optimizer_path(&def.file)
            || is_py_inflator_call_only_path(&def.file)
            || is_py_inflator_denominator_path(&def.file)
            || is_py_base_subtree_only(&def.file)
            || (is_py_repo_root_subtree(&def.file, &["ops"])
                && !is_py_optimizer_experiment_path(&def.file)))
    {
        return def
            .line
            .saturating_add(OPTIMIZER_CLASS_HEADER_LINES.saturating_sub(1))
            .min(def.end_line);
    }
    if (is_py_inflator_calibration_path(&def.file)
        || is_py_inflator_denominator_path(&def.file)
        || is_py_base_subtree_only(&def.file))
        && matches!(
            def.kind,
            crate::units::CodeUnitKind::Function | crate::units::CodeUnitKind::Method
        )
    {
        return def
            .line
            .saturating_add(INFLATOR_FUNCTION_HEADER_LINES.saturating_sub(1))
            .min(def.end_line);
    }
    def.end_line
}

pub(crate) fn path_normal_components(path: &std::path::Path) -> Vec<&str> {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect()
}

const REPO_SUBTREE_PARENT_BLOCKLIST: &[&str] = &[
    "widgets", "lib", "reports", "src", "tests", "test", "base", "contrib", "refactor",
];

/// Repo-root subtrees keyed by first path segment (works for relative paths and absolute repo paths).
pub(crate) fn is_py_repo_root_subtree(path: &std::path::Path, subtrees: &[&str]) -> bool {
    let comps = path_normal_components(path);
    if comps.first().is_some_and(|seg| subtrees.contains(seg)) {
        return true;
    }
    comps.windows(2).any(|pair| {
        subtrees.contains(&pair[1]) && !REPO_SUBTREE_PARENT_BLOCKLIST.contains(&pair[0])
    })
}

const NON_PACKAGE_VOID_PARENTS: &[&str] = &["widgets", "lib", "reports", "src", "tests", "test"];

/// `PKG/{base,contrib,refactor}/…` package-root subtrees only (not arbitrary nested `base/` segments).
pub(crate) fn is_py_contrib_base_void_partition(path: &std::path::Path) -> bool {
    let comps: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    if comps
        .get(1)
        .is_some_and(|seg| matches!(*seg, "base" | "contrib" | "refactor"))
    {
        return true;
    }
    comps.windows(2).any(|pair| {
        matches!(pair[1], "base" | "contrib" | "refactor")
            && !NON_PACKAGE_VOID_PARENTS.contains(&pair[0])
    })
}

/// `PKG/base/oi/…` — object-inference subtree; integration tests reach via package imports.
pub fn is_py_base_oi_subtree(path: &std::path::Path) -> bool {
    if !is_py_base_subtree_only(path) {
        return false;
    }
    path_normal_components(path)
        .windows(2)
        .any(|pair| pair[0] == "base" && pair[1] == "oi")
}

/// `PKG/base/…` package-root subtree only (not contrib/refactor void partitions).
pub(crate) fn is_py_base_subtree_only(path: &std::path::Path) -> bool {
    let comps = path_normal_components(path);
    if comps.get(1).is_some_and(|seg| *seg == "base") {
        return true;
    }
    comps.windows(2).any(|pair| {
        pair[1] == "base" && !NON_PACKAGE_VOID_PARENTS.contains(&pair[0])
    })
}

/// `contrib/` and `refactor/` subtrees: force uncovered in calibration (llvm runs only a thin
/// subset; static import witnesses over-credit). `base/` is excluded — tests import core APIs.
pub(crate) fn is_py_contrib_refactor_void_force_uncovered(path: &std::path::Path) -> bool {
    let comps = path_normal_components(path);
    if comps
        .get(1)
        .is_some_and(|seg| matches!(*seg, "contrib" | "refactor"))
    {
        return true;
    }
    comps.windows(2).any(|pair| {
        matches!(pair[1], "contrib" | "refactor")
            && !NON_PACKAGE_VOID_PARENTS.contains(&pair[0])
    })
}

/// Repo-root `optimizer/` only (not `experiments/` — modal scripts inflate under import-cal).
pub(crate) fn is_py_optimizer_path(path: &std::path::Path) -> bool {
    is_py_repo_root_subtree(path, &["optimizer"])
}

/// Repo-root `experiments/` only: modal scripts inflate under import-cal strict tier.
pub(crate) fn is_py_experiments_path(path: &std::path::Path) -> bool {
    is_py_repo_root_subtree(path, &["experiments"])
}

/// Repo-root `optimizer/` / `experiments/` trees: per-def import-cal credit (no module-level collapse).
pub(crate) fn is_py_optimizer_experiment_path(path: &std::path::Path) -> bool {
    is_py_repo_root_subtree(path, &["optimizer", "experiments"])
}

/// Repo-root `acq/` subtrees: conftest import cones over-credit acquisition modules.
pub(crate) fn is_py_acq_subtree(path: &std::path::Path) -> bool {
    is_py_repo_root_subtree(path, &["acq"])
}

/// Repo-root inflator subtrees: conftest import cones over-credit large class bodies.
pub(crate) fn is_py_inflator_calibration_path(path: &std::path::Path) -> bool {
    is_py_repo_root_subtree(path, &["optimizer", "experiments", "analysis", "ops"])
}

/// Inflator paths where def bodies belong in the coverage denominator but header-only credit.
pub fn is_py_inflator_denominator_path(path: &std::path::Path) -> bool {
    is_py_repo_root_subtree(path, &["experiments", "ops"])
}

/// Repo-root `rl/` integration subtrees: pytest executes via facade imports; allow expanded reach.
pub(crate) fn is_py_rl_integration_path(path: &std::path::Path) -> bool {
    is_py_repo_root_subtree(path, &["rl"])
}

/// Workspace-adjacent ecosystem packages (e.g. `ruff-ecosystem/`): static import witnesses
/// over-credit tooling that llvm/slipcover does not execute in the unit-test run.
pub(crate) fn is_py_ecosystem_auxiliary_path(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(s) if s
                .to_str()
                .is_some_and(|n| n.ends_with("-ecosystem"))
        )
    })
}

/// Repo-root optimizer/analysis only: call-only witnesses (ops/experiments need expanded reach).
pub(crate) fn is_py_inflator_call_only_path(path: &std::path::Path) -> bool {
    is_py_repo_root_subtree(path, &["optimizer", "analysis"])
}

use crate::test_refs::CodeDefinition;
use crate::test_refs::detection::is_python_test_file;
use crate::parsing::ParsedFile;
use std::collections::HashSet;
use std::path::PathBuf;
use tree_sitter::Node;

/// Quoted `ops/foo.py` / `rl/pkg/mod.py` paths in test sources attest every def in matching files.
pub(crate) fn expand_py_path_literal_file_witnesses(
    parsed_files: &[&ParsedFile],
    definitions: &[CodeDefinition],
    refs: &mut HashSet<String>,
) {
    let mut path_literals = HashSet::new();
    for parsed in parsed_files {
        if !is_python_test_file(parsed) {
            continue;
        }
        collect_py_path_string_literals(parsed.tree.root_node(), &parsed.source, &mut path_literals);
    }
    if path_literals.is_empty() {
        return;
    }
    for def in definitions {
        let file = def.file.to_string_lossy();
        if path_literals
            .iter()
            .any(|lit| file.ends_with(lit.as_os_str().to_string_lossy().as_ref()))
        {
            refs.insert(def.name.clone());
        }
    }
}

pub(crate) fn collect_py_path_string_literals(node: Node, source: &str, out: &mut HashSet<PathBuf>) {
    if node.kind() == "string" {
        let raw = &source[node.start_byte()..node.end_byte()];
        let inner = raw
            .trim()
            .trim_matches(|c| c == '"' || c == '\'');
        if inner.ends_with(".py")
            && inner.contains('/')
            && !inner.contains("..")
            && inner.chars().all(|c| c.is_ascii() && !c.is_whitespace())
        {
            out.insert(PathBuf::from(inner));
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_py_path_string_literals(child, source, out);
    }
}

pub(crate) fn unquote_py_string(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string()
}
