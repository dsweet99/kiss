use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use std::collections::HashSet;
use syn::Item;

pub fn build_rust_dependency_graph(parsed_files: &[&ParsedRustFile]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();
    let mut internal_modules = HashSet::new();

    for parsed in parsed_files {
        let module_name = parsed.path.file_stem().map_or_else(
            || String::from("unknown"),
            |s| s.to_string_lossy().into_owned(),
        );
        internal_modules.insert(module_name.clone());
        graph.paths.insert(module_name.clone(), parsed.path.clone());
        graph.get_or_create_node(&module_name);
    }

    for parsed in parsed_files {
        let module_name = parsed.path.file_stem().map_or_else(
            || String::from("unknown"),
            |s| s.to_string_lossy().into_owned(),
        );
        let imports = extract_rust_imports(&parsed.ast);

        for import in imports {
            if internal_modules.contains(&import) {
                graph.add_dependency(&module_name, &import);
            }
        }
    }

    graph
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
}
