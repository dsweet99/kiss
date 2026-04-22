use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use syn::Item;

#[cfg(test)]
mod tests;

/// Compute a qualified Rust module name from a file path.
///
/// Mirrors the Python `qualified_module_name` to avoid collisions
/// when two files in different directories share the same stem
/// (e.g. `foo/utils.rs` and `bar/utils.rs`).
///
/// - `src/foo.rs`       → `"foo"`
/// - `src/foo/bar.rs`   → `"foo.bar"`
/// - `src/foo/mod.rs`   → `"foo"`   (mod.rs represents its parent)
pub(crate) fn qualified_rust_module_name(path: &Path) -> String {
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
        graph
            .path_to_module
            .insert(parsed.path.clone(), qualified.clone());
        graph.paths.insert(qualified.clone(), parsed.path.clone());
        graph.get_or_create_node(&qualified);
    }

    for parsed in parsed_files {
        let module_name = qualified_rust_module_name(&parsed.path);
        let imports = extract_rust_imports(&parsed.ast);

        // `mod foo;` declares a child module relative to this module (except at crate roots).
        for child in imports.mod_decls {
            let expected = qualify_child_module(&module_name, &child);
            if internal_modules.contains(&expected) {
                graph.add_dependency(&module_name, &expected);
            } else {
                // Fallback: treat as a bare import.
                resolve_import(
                    &child,
                    &module_name,
                    &internal_modules,
                    &bare_to_qualified,
                    &mut graph,
                );
            }
        }

        for import in imports.use_roots {
            resolve_import(
                &import,
                &module_name,
                &internal_modules,
                &bare_to_qualified,
                &mut graph,
            );
        }
    }

    graph
}

fn qualify_child_module(parent_module: &str, child: &str) -> String {
    // For `src/lib.rs` and `src/main.rs`, module names are "lib"/"main" in this graph,
    // but `mod foo;` declares top-level module "foo", not "lib.foo"/"main.foo".
    if matches!(parent_module, "lib" | "main" | "build") {
        child.to_string()
    } else {
        format!("{parent_module}.{child}")
    }
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

pub(crate) struct RustImports {
    pub(crate) use_roots: Vec<String>,
    pub(crate) mod_decls: Vec<String>,
}

pub(crate) fn extract_rust_imports(ast: &syn::File) -> RustImports {
    let mut use_roots = Vec::new();
    let mut mod_decls = Vec::new();
    extract_imports_from_items(&ast.items, &mut use_roots, &mut mod_decls);
    RustImports {
        use_roots,
        mod_decls,
    }
}

// Recursively extract imports from all scopes (matching Python behavior)
pub(crate) fn extract_imports_from_items(
    items: &[Item],
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
) {
    for item in items {
        match item {
            Item::Use(use_item) => collect_use_paths(&use_item.tree, use_roots),
            // include!("path/to/file.rs") textual inclusion; treat as a file dependency.
            Item::Macro(item_macro) => {
                if let Some(stem) = extract_include_rs_stem(&item_macro.mac) {
                    mod_decls.push(stem);
                }
            }
            // mod foo; (external file reference) creates a dependency edge
            Item::Mod(mod_item) if mod_item.content.is_none() => {
                mod_decls.push(mod_item.ident.to_string());
            }
            // Recurse into inline modules
            Item::Mod(mod_item) if mod_item.content.is_some() => {
                if let Some((_, items)) = &mod_item.content {
                    extract_imports_from_items(items, use_roots, mod_decls);
                }
            }
            // Recurse into function bodies
            Item::Fn(fn_item) => extract_imports_from_block(&fn_item.block, use_roots, mod_decls),
            // Recurse into impl blocks
            Item::Impl(impl_item) => {
                for impl_item in &impl_item.items {
                    if let syn::ImplItem::Fn(method) = impl_item {
                        extract_imports_from_block(&method.block, use_roots, mod_decls);
                    }
                }
            }
            // Recurse into trait definitions (default method bodies)
            Item::Trait(trait_item) => {
                for trait_item in &trait_item.items {
                    if let syn::TraitItem::Fn(method) = trait_item
                        && let Some(block) = &method.default
                    {
                        extract_imports_from_block(block, use_roots, mod_decls);
                    }
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn extract_imports_from_block(
    block: &syn::Block,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
) {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Item(item) => {
                extract_imports_from_items(std::slice::from_ref(item), use_roots, mod_decls);
            }
            syn::Stmt::Expr(expr, _) => extract_imports_from_expr(expr, use_roots, mod_decls),
            syn::Stmt::Local(syn::Local {
                init: Some(init), ..
            }) => {
                extract_imports_from_expr(&init.expr, use_roots, mod_decls);
            }
            _ => {}
        }
    }
}

pub(crate) fn extract_imports_from_expr(
    expr: &syn::Expr,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
) {
    // Handle closures and async blocks that can contain use statements
    match expr {
        syn::Expr::Block(block) => extract_imports_from_block(&block.block, use_roots, mod_decls),
        syn::Expr::Async(async_block) => {
            extract_imports_from_block(&async_block.block, use_roots, mod_decls);
        }
        syn::Expr::Macro(m) => {
            if let Some(stem) = extract_include_rs_stem(&m.mac) {
                mod_decls.push(stem);
            }
        }
        syn::Expr::Closure(closure) => {
            if let syn::Expr::Block(block) = &*closure.body {
                extract_imports_from_block(&block.block, use_roots, mod_decls);
            }
        }
        syn::Expr::If(if_expr) => {
            extract_imports_from_block(&if_expr.then_branch, use_roots, mod_decls);
            if let Some((_, else_branch)) = &if_expr.else_branch {
                extract_imports_from_expr(else_branch, use_roots, mod_decls);
            }
        }
        syn::Expr::Loop(loop_expr) => {
            extract_imports_from_block(&loop_expr.body, use_roots, mod_decls);
        }
        syn::Expr::While(while_expr) => {
            extract_imports_from_block(&while_expr.body, use_roots, mod_decls);
        }
        syn::Expr::ForLoop(for_expr) => {
            extract_imports_from_block(&for_expr.body, use_roots, mod_decls);
        }
        syn::Expr::Match(match_expr) => {
            for arm in &match_expr.arms {
                extract_imports_from_expr(&arm.body, use_roots, mod_decls);
            }
        }
        syn::Expr::Unsafe(unsafe_expr) => {
            extract_imports_from_block(&unsafe_expr.block, use_roots, mod_decls);
        }
        _ => {}
    }
}

fn extract_include_rs_stem(mac: &syn::Macro) -> Option<String> {
    // Support `include!("path/to/foo.rs")` by treating it as a dependency edge to `foo.rs`.
    // This matches kiss's module-per-file graph model, even though include! is textual inclusion.
    if !mac.path.is_ident("include") {
        return None;
    }
    let lit: syn::LitStr = syn::parse2(mac.tokens.clone()).ok()?;
    let path = lit.value();
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(path.as_str());
    let stem = filename.strip_suffix(".rs").unwrap_or(filename);
    (!stem.is_empty()).then(|| stem.to_string())
}

pub(crate) fn collect_use_paths(tree: &syn::UseTree, imports: &mut Vec<String>) {
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
