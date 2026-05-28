/// Class defs on optimizer paths: header/docstring lines only (constructor witness must not
/// credit entire class bodies vs slipcover).
#[allow(dead_code)]
pub(crate) const OPTIMIZER_CLASS_HEADER_LINES: usize = 3;

pub fn calibration_def_end_line(def: &CodeDefinition) -> usize {
    def.end_line
}

/// `base/`, `contrib/`, and `refactor/` subtrees: skip directory-sibling and production-import expansion.
pub(crate) fn is_py_contrib_base_void_partition(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(s)
                if s.to_str().is_some_and(|n| n == "base" || n == "contrib" || n == "refactor")
        )
    })
}

/// `base/`, `contrib/`, and `refactor/` subtrees: force uncovered in calibration (llvm runs
/// only a thin subset; static import witnesses over-credit).
pub(crate) fn is_py_contrib_refactor_void_force_uncovered(path: &std::path::Path) -> bool {
    is_py_contrib_base_void_partition(path)
}

/// Optimizer/experiment subtrees: per-def import-cal credit (no module-level collapse).
pub(crate) fn is_py_optimizer_experiment_path(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(s)
                if s.to_str().is_some_and(|n| n == "optimizer" || n == "experiments")
        )
    })
}

/// Yubo-style inflator subtrees: conftest import cones over-credit large class bodies.
pub(crate) fn is_py_inflator_calibration_path(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(s)
                if s.to_str().is_some_and(|n| {
                    n == "optimizer" || n == "experiments" || n == "analysis" || n == "ops"
                })
        )
    })
}

/// Optimizer/analysis only: call-only witnesses (ops/experiments need expanded reach for blind spots).
#[allow(dead_code)]
pub(crate) fn is_py_inflator_call_only_path(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(s)
                if s.to_str().is_some_and(|n| n == "optimizer" || n == "analysis")
        )
    })
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
