use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::graph::{
    extract_dynamic_import_module, extract_imports_for_cache, is_dunder_import,
    is_importlib_import_module, qualified_module_name,
};
use crate::parsing::ParsedFile;

use super::error::RoleBuildError;
use super::facts::{FileRoleFacts, RoleRange};
use super::index::SourceRoleIndex;
use super::python_path::is_python_test_module_path;
use super::span::SourceSpan;
use super::sweep::normalize_ranges;
use super::types::CodeContextSet;

pub fn classify_python(
    parsed: &[&ParsedFile],
    discovered: &[PathBuf],
) -> Result<SourceRoleIndex, RoleBuildError> {
    let mut index = SourceRoleIndex::empty();
    let modules = python_module_map(parsed);
    let imports = python_import_edges(parsed);
    let unresolved_dynamic = production_unresolved_dynamic(parsed, &modules);
    let mut contexts = seed_python_contexts(parsed);
    if unresolved_dynamic {
        promote_all_test_named(&mut contexts, parsed);
    }
    propagate_python_contexts(&mut contexts, &imports);
    for parsed in parsed {
        let ctx = contexts
            .get(&canonical_key(&parsed.path))
            .copied()
            .unwrap_or_else(CodeContextSet::production_only);
        let span = SourceSpan::whole_file(&parsed.source);
        let ranges = normalize_ranges(vec![RoleRange {
            span,
            contexts: ctx,
        }]);
        index.insert(parsed.path.clone(), FileRoleFacts::new(ctx, ranges));
    }
    for path in discovered {
        if index.file_composition(path) == super::types::FileComposition::ProductionOnly
            && !parsed
                .iter()
                .any(|p| canonical_key(&p.path) == canonical_key(path))
        {
            index.insert(
                path.clone(),
                FileRoleFacts::new(CodeContextSet::production_only(), Vec::new()),
            );
        }
    }
    Ok(index)
}

fn canonical_key(path: &Path) -> PathBuf {
    crate::rust_include::canonical_path(path)
}

fn python_module_map(parsed: &[&ParsedFile]) -> HashMap<String, Vec<PathBuf>> {
    let mut map: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for file in parsed {
        let name = qualified_module_name(&file.path);
        map.entry(name).or_default().push(canonical_key(&file.path));
        if let Some(stem) = file.path.file_stem().and_then(|s| s.to_str()) {
            map.entry(stem.to_string())
                .or_default()
                .push(canonical_key(&file.path));
        }
    }
    map
}

fn python_import_edges(parsed: &[&ParsedFile]) -> HashMap<PathBuf, Vec<PathBuf>> {
    let modules = python_module_map(parsed);
    let mut edges: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for file in parsed {
        let from = canonical_key(&file.path);
        let names = extract_imports_for_cache(file.tree.root_node(), &file.source);
        let mut targets = Vec::new();
        for name in names {
            if let Some(paths) = modules.get(&name) {
                targets.extend(paths.iter().cloned());
            } else if let Some(head) = name.split('.').next()
                && let Some(paths) = modules.get(head)
            {
                targets.extend(paths.iter().cloned());
            }
        }
        edges.insert(from, targets);
    }
    edges
}

fn seed_python_contexts(parsed: &[&ParsedFile]) -> HashMap<PathBuf, CodeContextSet> {
    let mut contexts = HashMap::new();
    for file in parsed {
        let ctx = if is_python_test_module_path(&file.path) {
            CodeContextSet::test_only()
        } else {
            CodeContextSet::production_only()
        };
        contexts.insert(canonical_key(&file.path), ctx);
    }
    contexts
}

fn production_unresolved_dynamic(
    parsed: &[&ParsedFile],
    _modules: &HashMap<String, Vec<PathBuf>>,
) -> bool {
    parsed
        .iter()
        .any(|file| !is_python_test_module_path(&file.path) && has_unresolved_dynamic(file))
}

fn has_unresolved_dynamic(file: &ParsedFile) -> bool {
    walk_unresolved(file.tree.root_node(), &file.source)
}

fn walk_unresolved(node: tree_sitter::Node<'_>, source: &str) -> bool {
    if node.kind() == "call" && call_is_unresolved_dynamic(node, source) {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| walk_unresolved(child, source))
}

fn call_is_unresolved_dynamic(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let Some(func) = node.child_by_field_name("function") else {
        return false;
    };
    let is_dyn = is_importlib_import_module(func, source) || is_dunder_import(func, source);
    is_dyn && extract_dynamic_import_module(node, source).is_none()
}

fn promote_all_test_named(contexts: &mut HashMap<PathBuf, CodeContextSet>, parsed: &[&ParsedFile]) {
    for file in parsed {
        if is_python_test_module_path(&file.path) {
            let key = canonical_key(&file.path);
            if let Some(ctx) = contexts.get_mut(&key) {
                ctx.production = true;
            }
        }
    }
}

fn propagate_python_contexts(
    contexts: &mut HashMap<PathBuf, CodeContextSet>,
    edges: &HashMap<PathBuf, Vec<PathBuf>>,
) {
    let mut queue: VecDeque<PathBuf> = contexts.keys().cloned().collect();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    while let Some(from) = queue.pop_front() {
        if !seen.insert(from.clone()) && queue.len() > contexts.len().saturating_mul(4) {
            continue;
        }
        let from_ctx = contexts
            .get(&from)
            .copied()
            .unwrap_or_else(CodeContextSet::none);
        let Some(targets) = edges.get(&from) else {
            continue;
        };
        for target in targets {
            let entry = contexts
                .entry(target.clone())
                .or_insert_with(CodeContextSet::none);
            let before = *entry;
            *entry = entry.union(from_ctx);
            if *entry != before {
                seen.remove(target);
                queue.push_back(target.clone());
            }
        }
    }
}

#[cfg(test)]
mod python_roles_test {
    use super::*;
    use crate::code_roles::types::CodeRole;
    use crate::parsing::{create_parser, parse_file};

    fn parse_one(dir: &Path, name: &str, src: &str) -> ParsedFile {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, src).unwrap();
        let mut parser = create_parser().unwrap();
        parse_file(&mut parser, &path).unwrap()
    }

    #[test]
    fn test_named_modules_are_test_only() {
        let tmp = tempfile::tempdir().unwrap();
        let helpers = parse_one(tmp.path(), "tests/helpers.py", "X = 1\n");
        let test_foo = parse_one(tmp.path(), "test_foo.py", "def test_a():\n    pass\n");
        let prod = parse_one(tmp.path(), "src/foo.py", "def f():\n    return 1\n");
        let parsed = vec![&helpers, &test_foo, &prod];
        let index = classify_python(&parsed, &[]).unwrap();
        assert_eq!(index.role_at(&helpers.path, 1), CodeRole::TestOnly);
        assert_eq!(index.role_at(&test_foo.path, 1), CodeRole::TestOnly);
        assert_eq!(index.role_at(&prod.path, 1), CodeRole::Production);
    }

    #[test]
    fn production_import_promotes_test_named_module() {
        let tmp = tempfile::tempdir().unwrap();
        let helper = parse_one(tmp.path(), "test_helper.py", "VAL = 1\n");
        let prod = parse_one(tmp.path(), "app.py", "import test_helper\n");
        let parsed = vec![&helper, &prod];
        let index = classify_python(&parsed, &[]).unwrap();
        assert_eq!(index.role_at(&helper.path, 1), CodeRole::Production);
    }
}
