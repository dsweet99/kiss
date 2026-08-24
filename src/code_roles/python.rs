use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use rayon::prelude::*;

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
    let modules = python_module_map(parsed, discovered);
    let imports = python_import_edges(parsed, &modules);
    let mut contexts = seed_python_contexts(parsed);
    seed_unparsed_discovered(&mut contexts, parsed, discovered);
    if production_unresolved_dynamic(parsed) {
        promote_all_test_named(&mut contexts, parsed, discovered);
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
    insert_unparsed_discovered(&mut index, parsed, discovered, &contexts);
    Ok(index)
}

fn insert_unparsed_discovered(
    index: &mut SourceRoleIndex,
    parsed: &[&ParsedFile],
    discovered: &[PathBuf],
    contexts: &HashMap<PathBuf, CodeContextSet>,
) {
    let parsed_keys: HashSet<PathBuf> = parsed.iter().map(|p| canonical_key(&p.path)).collect();
    for path in discovered {
        let key = canonical_key(path);
        if parsed_keys.contains(&key) {
            continue;
        }
        let ctx = contexts
            .get(&key)
            .copied()
            .unwrap_or_else(|| path_seed_context(path));
        index.insert(path.clone(), FileRoleFacts::new(ctx, Vec::new()));
    }
}

fn path_seed_context(path: &Path) -> CodeContextSet {
    if is_python_test_module_path(path) {
        CodeContextSet::test_only()
    } else {
        CodeContextSet::production_only()
    }
}

fn seed_unparsed_discovered(
    contexts: &mut HashMap<PathBuf, CodeContextSet>,
    parsed: &[&ParsedFile],
    discovered: &[PathBuf],
) {
    let parsed_keys: HashSet<PathBuf> = parsed.iter().map(|p| canonical_key(&p.path)).collect();
    for path in discovered {
        let key = canonical_key(path);
        if parsed_keys.contains(&key) {
            continue;
        }
        contexts
            .entry(key)
            .or_insert_with(|| path_seed_context(path));
    }
}

fn canonical_key(path: &Path) -> PathBuf {
    crate::rust_include::canonical_path(path)
}

fn python_module_map(
    parsed: &[&ParsedFile],
    discovered: &[PathBuf],
) -> HashMap<String, Vec<PathBuf>> {
    let mut map: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for file in parsed {
        add_module_aliases(&mut map, &file.path);
    }
    let parsed_keys: HashSet<PathBuf> = parsed.iter().map(|p| canonical_key(&p.path)).collect();
    for path in discovered {
        if parsed_keys.contains(&canonical_key(path)) {
            continue;
        }
        add_module_aliases(&mut map, path);
    }
    map
}

fn add_module_aliases(map: &mut HashMap<String, Vec<PathBuf>>, path: &Path) {
    let name = qualified_module_name(path);
    map.entry(name).or_default().push(canonical_key(path));
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        map.entry(stem.to_string())
            .or_default()
            .push(canonical_key(path));
    }
}

fn python_import_edges(
    parsed: &[&ParsedFile],
    modules: &HashMap<String, Vec<PathBuf>>,
) -> HashMap<PathBuf, Vec<PathBuf>> {
    parsed
        .par_iter()
        .map(|file| {
            let from = canonical_key(&file.path);
            let names = extract_imports_for_cache(file.tree.root_node(), &file.source);
            (from, resolve_python_import_targets(&names, modules))
        })
        .collect()
}

fn resolve_python_import_targets(
    names: &[String],
    modules: &HashMap<String, Vec<PathBuf>>,
) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for name in names {
        if let Some(paths) = modules.get(name) {
            targets.extend(paths.iter().cloned());
        } else if let Some(head) = name.split('.').next()
            && let Some(paths) = modules.get(head)
        {
            targets.extend(paths.iter().cloned());
        }
    }
    targets
}

fn production_unresolved_dynamic(parsed: &[&ParsedFile]) -> bool {
    parsed
        .iter()
        .any(|file| !is_python_test_module_path(&file.path) && has_unresolved_dynamic(file))
}

fn has_unresolved_dynamic(file: &ParsedFile) -> bool {
    let src = &file.source;
    if !src.contains("import_module") && !src.contains("__import__") {
        return false;
    }
    walk_unresolved(file.tree.root_node(), src)
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

fn promote_all_test_named(
    contexts: &mut HashMap<PathBuf, CodeContextSet>,
    parsed: &[&ParsedFile],
    discovered: &[PathBuf],
) {
    for path in parsed
        .iter()
        .map(|file| file.path.as_path())
        .chain(discovered.iter().map(PathBuf::as_path))
    {
        if is_python_test_module_path(path)
            && let Some(ctx) = contexts.get_mut(&canonical_key(path))
        {
            ctx.production = true;
        }
    }
}

fn seed_python_contexts(parsed: &[&ParsedFile]) -> HashMap<PathBuf, CodeContextSet> {
    let mut contexts = HashMap::new();
    for file in parsed {
        contexts.insert(canonical_key(&file.path), path_seed_context(&file.path));
    }
    contexts
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

    #[test]
    fn production_import_promotes_unparsed_test_named_discovered() {
        let tmp = tempfile::tempdir().unwrap();
        let helper = tmp.path().join("test_helper.py");
        std::fs::write(&helper, "VAL = 1\n").unwrap();
        let prod = parse_one(tmp.path(), "app.py", "import test_helper\n");
        let discovered = [helper.clone(), prod.path.clone()];
        let index = classify_python(&[&prod], &discovered).unwrap();
        assert_eq!(index.role_at(&helper, 1), CodeRole::Production);
    }

    #[test]
    fn discovered_file_without_parse_is_production() {
        let tmp = tempfile::tempdir().unwrap();
        let parsed = parse_one(tmp.path(), "app.py", "x = 1\n");
        let extra = tmp.path().join("only_discovered.py");
        std::fs::write(&extra, "y = 2\n").unwrap();
        let index = classify_python(&[&parsed], std::slice::from_ref(&extra)).unwrap();
        assert_eq!(index.role_at(&extra, 1), CodeRole::Production);
        assert_eq!(
            index.file_composition(&extra),
            super::super::types::FileComposition::ProductionOnly
        );
    }

    #[test]
    fn discovered_unparsed_test_file_is_test_only() {
        let tmp = tempfile::tempdir().unwrap();
        let parsed = parse_one(tmp.path(), "app.py", "x = 1\n");
        let extra = tmp.path().join("test_only_discovered.py");
        std::fs::write(&extra, "def test_a():\n    pass\n").unwrap();
        let index = classify_python(&[&parsed], std::slice::from_ref(&extra)).unwrap();
        assert_eq!(index.role_at(&extra, 1), CodeRole::TestOnly);
    }

    #[test]
    fn discovered_parsed_test_file_stays_test_only() {
        let tmp = tempfile::tempdir().unwrap();
        let test_foo = parse_one(tmp.path(), "test_foo.py", "def test_a():\n    pass\n");
        let files = [test_foo.path.clone()];
        let index = classify_python(&[&test_foo], &files).unwrap();
        assert_eq!(index.role_at(&test_foo.path, 1), CodeRole::TestOnly);
    }

    #[test]
    fn unresolved_dynamic_promotes_all_test_named() {
        let tmp = tempfile::tempdir().unwrap();
        let helper = parse_one(tmp.path(), "test_helper.py", "VAL = 1\n");
        let prod = parse_one(
            tmp.path(),
            "app.py",
            "import importlib\nmod = importlib.import_module(name)\n",
        );
        let index = classify_python(&[&helper, &prod], &[]).unwrap();
        assert_eq!(index.role_at(&helper.path, 1), CodeRole::Production);
        assert_eq!(index.role_at(&prod.path, 1), CodeRole::Production);
    }
}
