use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use syn::Item;

/// Compute a qualified Rust module name from a file path.
///
/// Mirrors the Python `qualified_module_name` to avoid collisions
/// when two files in different directories share the same stem
/// (e.g. `foo/utils.rs` and `bar/utils.rs`).
///
/// - `src/foo.rs`       → `"foo"`
/// - `src/foo/bar.rs`   → `"foo.bar"`
/// - `src/foo/mod.rs`   → `"foo"`   (mod.rs represents its parent)
fn qualified_rust_module_name(path: &Path) -> String {
    use std::path::Component;

    let stem = path
        .file_stem()
        .map_or("unknown", |s| s.to_str().unwrap_or("unknown"));

    let mut dirs: Vec<String> = path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    Component::Normal(os) => os.to_str().map(std::string::ToString::to_string),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Strip everything up to and including the last "src" or "tests" segment
    // (both are standard Rust source roots).
    if let Some(pos) = dirs.iter().rposition(|d| d == "src" || d == "tests") {
        dirs = dirs[(pos + 1)..].to_vec();
    }

    // For absolute paths without a known source root, keep a short tail.
    if path.is_absolute() && dirs.len() > 2 {
        dirs = dirs[(dirs.len() - 2)..].to_vec();
    }

    // mod.rs represents the parent directory, not itself.
    if stem == "mod" {
        if dirs.is_empty() {
            return "mod".to_string();
        }
        return dirs.join(".");
    }

    if dirs.is_empty() {
        stem.to_string()
    } else {
        format!("{}.{}", dirs.join("."), stem)
    }
}

pub fn build_rust_dependency_graph(parsed_files: &[&ParsedRustFile]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();
    let mut internal_modules = HashSet::new();
    // Map bare module stems to qualified names for import resolution.
    // Rust `use` statements reference crate-level names, so imports
    // appear as bare stems (e.g. `utils`) that we need to map to the
    // qualified graph key (e.g. `foo.utils`).
    let mut bare_to_qualified: HashMap<String, Vec<String>> = HashMap::new();

    for parsed in parsed_files {
        let qualified = qualified_rust_module_name(&parsed.path);
        let bare = parsed.path.file_stem().map_or_else(
            || String::from("unknown"),
            |s| s.to_string_lossy().into_owned(),
        );
        internal_modules.insert(qualified.clone());
        bare_to_qualified
            .entry(bare)
            .or_default()
            .push(qualified.clone());
        graph.paths.insert(qualified.clone(), parsed.path.clone());
        graph.get_or_create_node(&qualified);
    }

    for parsed in parsed_files {
        let module_name = qualified_rust_module_name(&parsed.path);
        let imports = extract_rust_imports(&parsed.ast);

        for import in imports {
            resolve_import(&import, &module_name, &internal_modules, &bare_to_qualified, &mut graph);
        }
    }

    graph
}

fn resolve_import(
    import: &str,
    module_name: &str,
    internal_modules: &HashSet<String>,
    bare_to_qualified: &HashMap<String, Vec<String>>,
    graph: &mut DependencyGraph,
) {
    if internal_modules.contains(import) {
        graph.add_dependency(module_name, import);
        return;
    }
    let Some(qualified_names) = bare_to_qualified.get(import) else {
        return;
    };
    for qualified in qualified_names {
        if qualified != module_name {
            graph.add_dependency(module_name, qualified);
        }
    }
}

fn extract_rust_imports(ast: &syn::File) -> Vec<String> {
    let mut imports = Vec::new();
    extract_imports_from_items(&ast.items, &mut imports);
    imports
}

// Recursively extract imports from all scopes (matching Python behavior)
fn extract_imports_from_items(items: &[Item], imports: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Use(use_item) => collect_use_paths(&use_item.tree, imports),
            // mod foo; (external file reference) creates a dependency edge
            Item::Mod(mod_item) if mod_item.content.is_none() => {
                imports.push(mod_item.ident.to_string());
            }
            // Recurse into inline modules
            Item::Mod(mod_item) if mod_item.content.is_some() => {
                if let Some((_, items)) = &mod_item.content {
                    extract_imports_from_items(items, imports);
                }
            }
            // Recurse into function bodies
            Item::Fn(fn_item) => extract_imports_from_block(&fn_item.block, imports),
            // Recurse into impl blocks
            Item::Impl(impl_item) => {
                for impl_item in &impl_item.items {
                    if let syn::ImplItem::Fn(method) = impl_item {
                        extract_imports_from_block(&method.block, imports);
                    }
                }
            }
            // Recurse into trait definitions (default method bodies)
            Item::Trait(trait_item) => {
                for trait_item in &trait_item.items {
                    if let syn::TraitItem::Fn(method) = trait_item
                        && let Some(block) = &method.default
                    {
                        extract_imports_from_block(block, imports);
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_imports_from_block(block: &syn::Block, imports: &mut Vec<String>) {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Item(item) => {
                extract_imports_from_items(std::slice::from_ref(item), imports);
            }
            syn::Stmt::Expr(expr, _) => extract_imports_from_expr(expr, imports),
            syn::Stmt::Local(syn::Local {
                init: Some(init), ..
            }) => {
                extract_imports_from_expr(&init.expr, imports);
            }
            _ => {}
        }
    }
}

fn extract_imports_from_expr(expr: &syn::Expr, imports: &mut Vec<String>) {
    // Handle closures and async blocks that can contain use statements
    match expr {
        syn::Expr::Block(block) => extract_imports_from_block(&block.block, imports),
        syn::Expr::Async(async_block) => extract_imports_from_block(&async_block.block, imports),
        syn::Expr::Closure(closure) => {
            if let syn::Expr::Block(block) = &*closure.body {
                extract_imports_from_block(&block.block, imports);
            }
        }
        syn::Expr::If(if_expr) => {
            extract_imports_from_block(&if_expr.then_branch, imports);
            if let Some((_, else_branch)) = &if_expr.else_branch {
                extract_imports_from_expr(else_branch, imports);
            }
        }
        syn::Expr::Loop(loop_expr) => extract_imports_from_block(&loop_expr.body, imports),
        syn::Expr::While(while_expr) => extract_imports_from_block(&while_expr.body, imports),
        syn::Expr::ForLoop(for_expr) => extract_imports_from_block(&for_expr.body, imports),
        syn::Expr::Match(match_expr) => {
            for arm in &match_expr.arms {
                extract_imports_from_expr(&arm.body, imports);
            }
        }
        syn::Expr::Unsafe(unsafe_expr) => extract_imports_from_block(&unsafe_expr.block, imports),
        _ => {}
    }
}

fn collect_use_paths(tree: &syn::UseTree, imports: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(path) => {
            let crate_name = path.ident.to_string();
            if !matches!(crate_name.as_str(), "self" | "super" | "crate") {
                imports.push(crate_name);
            }
        }
        syn::UseTree::Name(name) => {
            let crate_name = name.ident.to_string();
            if !matches!(crate_name.as_str(), "self" | "super" | "crate") {
                imports.push(crate_name);
            }
        }
        syn::UseTree::Rename(rename) => {
            let crate_name = rename.ident.to_string();
            if !matches!(crate_name.as_str(), "self" | "super" | "crate") {
                imports.push(crate_name);
            }
        }
        syn::UseTree::Glob(_) => {}
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_paths(item, imports);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rust_code(code: &str) -> syn::File {
        syn::parse_file(code).expect("Failed to parse Rust code")
    }

    #[test]
    fn extracts_simple_use() {
        let ast = parse_rust_code("use std;");
        let imports = extract_rust_imports(&ast);
        assert!(
            imports.contains(&String::from("std")),
            "imports: {imports:?}"
        );
    }

    #[test]
    fn extracts_path_use() {
        let ast = parse_rust_code("use std::collections::HashMap;");
        let imports = extract_rust_imports(&ast);
        assert!(
            imports.contains(&String::from("std")),
            "imports: {imports:?}"
        );
    }

    #[test]
    fn extracts_multiple_uses() {
        let ast = parse_rust_code(
            r"
use std::io;
use serde::Serialize;
use crate::module;
",
        );
        let imports = extract_rust_imports(&ast);
        assert!(
            imports.contains(&String::from("std")),
            "imports: {imports:?}"
        );
        assert!(
            imports.contains(&String::from("serde")),
            "imports: {imports:?}"
        );
        assert!(
            !imports.contains(&String::from("crate")),
            "crate:: should be excluded"
        );
    }

    #[test]
    fn handles_grouped_uses() {
        let ast = parse_rust_code("use std::{io, collections::HashMap};");
        let imports = extract_rust_imports(&ast);
        assert!(
            imports.contains(&String::from("std")),
            "imports: {imports:?}"
        );
    }

    #[test]
    fn test_build_rust_dependency_graph() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "use std::io;").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let refs: Vec<&crate::rust_parsing::ParsedRustFile> = vec![&parsed];
        let graph = build_rust_dependency_graph(&refs);
        assert!(!graph.nodes.is_empty());
    }

    #[test]
    fn test_collect_use_paths() {
        let ast = parse_rust_code("use foo::bar;");
        let mut imports = Vec::new();
        if let syn::Item::Use(u) = &ast.items[0] {
            collect_use_paths(&u.tree, &mut imports);
        }
        assert!(imports.contains(&"foo".to_string()));
    }

    #[test]
    fn extracts_function_scoped_use() {
        // Function-scoped imports should be captured (matching Python behavior)
        let ast = parse_rust_code(
            r"
fn foo() {
    use std::fs;
    use serde::Serialize;
}
",
        );
        let imports = extract_rust_imports(&ast);
        assert!(
            imports.contains(&String::from("std")),
            "function-scoped use not found: {imports:?}"
        );
        assert!(
            imports.contains(&String::from("serde")),
            "function-scoped use not found: {imports:?}"
        );
    }

    #[test]
    fn extracts_impl_method_scoped_use() {
        let ast = parse_rust_code(
            r"
struct Foo;
impl Foo {
    fn bar() {
        use std::io;
    }
}
",
        );
        let imports = extract_rust_imports(&ast);
        assert!(
            imports.contains(&String::from("std")),
            "impl method use not found: {imports:?}"
        );
    }

    #[test]
    fn extracts_inline_module_use() {
        let ast = parse_rust_code(
            r"
mod inner {
    use tokio::runtime;
}
",
        );
        let imports = extract_rust_imports(&ast);
        assert!(
            imports.contains(&String::from("tokio")),
            "inline module use not found: {imports:?}"
        );
    }

    #[test]
    fn test_qualified_rust_module_name() {
        assert_eq!(
            qualified_rust_module_name(Path::new("src/foo.rs")),
            "foo"
        );
        assert_eq!(
            qualified_rust_module_name(Path::new("src/foo/bar.rs")),
            "foo.bar"
        );
        assert_eq!(
            qualified_rust_module_name(Path::new("src/foo/mod.rs")),
            "foo"
        );
        assert_eq!(
            qualified_rust_module_name(Path::new("utils.rs")),
            "utils"
        );
        assert_eq!(
            qualified_rust_module_name(Path::new("tests/integration/helpers.rs")),
            "integration.helpers"
        );
    }

    #[test]
    fn test_same_stem_different_dirs_no_collision() {
        use std::io::Write;
        // Two files with the same stem in different directories should have
        // distinct module identities in the graph.
        let tmp = tempfile::TempDir::new().unwrap();
        let dir_a = tmp.path().join("src").join("foo");
        let dir_b = tmp.path().join("src").join("bar");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();

        let path_a = dir_a.join("utils.rs");
        let path_b = dir_b.join("utils.rs");
        let mut fa = std::fs::File::create(&path_a).unwrap();
        let mut fb = std::fs::File::create(&path_b).unwrap();
        writeln!(fa, "pub fn a() {{}}").unwrap();
        writeln!(fb, "pub fn b() {{}}").unwrap();

        let pa = crate::rust_parsing::parse_rust_file(&path_a).unwrap();
        let pb = crate::rust_parsing::parse_rust_file(&path_b).unwrap();
        let refs: Vec<&crate::rust_parsing::ParsedRustFile> = vec![&pa, &pb];
        let graph = build_rust_dependency_graph(&refs);

        // Should have 2 distinct nodes, not 1 collapsed node
        assert_eq!(
            graph.nodes.len(),
            2,
            "Two files named utils.rs in different dirs should produce 2 graph nodes, got: {:?}",
            graph.nodes.keys().collect::<Vec<_>>()
        );
    }
}
