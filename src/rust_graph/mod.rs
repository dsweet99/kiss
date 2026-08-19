use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use std::collections::{HashMap, HashSet};
use std::path::Path;
mod extract_imports;
mod include_graph;
mod resolve;

#[cfg(test)]
pub(crate) use resolve::{qualify_child_module, resolve_import};

#[cfg(test)]
pub(crate) use extract_imports::{
    extract_imports_from_block, extract_imports_from_expr, extract_imports_from_items,
};

pub use include_graph::{IncludeGraph, build_include_graph, expand_rust_files};

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



    if let Some(pos) = dirs.iter().rposition(|d| d == "src" || d == "tests") {
        dirs = dirs[(pos + 1)..].to_vec();
    }


    if path.is_absolute() && dirs.len() > 2 {
        dirs = dirs[(dirs.len() - 2)..].to_vec();
    }


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
        let file_path = crate::rust_include::canonical_path(&parsed.path);
        graph
            .path_to_module
            .insert(file_path.clone(), qualified.clone());
        graph.paths.insert(qualified.clone(), file_path);
        graph.get_or_create_node(&qualified);
    }

    for parsed in parsed_files {
        let module_name = qualified_rust_module_name(&parsed.path);
        let imports = extract_rust_imports(&parsed.ast);

        for lit in &imports.include_literals {
            let target = crate::rust_include::resolve_include_path(&parsed.path, lit);
            let key = crate::rust_include::canonical_path(&target);
            if let Some(child_module) = graph.path_to_module.get(&key).cloned() {
                graph.add_dependency(&module_name, &child_module);
            }
        }


        for child in imports.mod_decls {
            let expected = resolve::qualify_child_module(&module_name, &child);
            if internal_modules.contains(&expected) {
                graph.add_dependency(&module_name, &expected);
            } else {

                resolve::resolve_import(
                    &child,
                    &module_name,
                    &internal_modules,
                    &bare_to_qualified,
                    &mut graph,
                );
            }
        }

        for import in imports.use_roots {
            resolve::resolve_import(
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

pub(crate) struct RustImports {
    pub(crate) use_roots: Vec<String>,
    pub(crate) mod_decls: Vec<String>,
    pub(crate) include_literals: Vec<String>,
}

pub(crate) fn extract_rust_imports(ast: &syn::File) -> RustImports {
    let mut use_roots = Vec::new();
    let mut mod_decls = Vec::new();
    let mut include_literals = Vec::new();
    extract_imports::extract_imports_from_items(
        &ast.items,
        &mut use_roots,
        &mut mod_decls,
        &mut include_literals,
    );
    RustImports {
        use_roots,
        mod_decls,
        include_literals,
    }
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

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl RustImports {
        fn witness() -> Self {
            Self {
                use_roots: vec![],
                mod_decls: vec![],
                include_literals: vec![],
            }
        }
    }

    #[test]
    fn witness_rust_imports() {
        let _ = RustImports::witness();
        let parsed = syn::parse_file("use std;").unwrap();
        let imports = extract_rust_imports(&parsed);
        assert_eq!(imports.use_roots, vec!["std".to_string()]);
    }
}
