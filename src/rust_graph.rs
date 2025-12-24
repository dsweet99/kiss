//! Dependency graph analysis for Rust

use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use syn::Item;

/// Build a dependency graph from parsed Rust files
pub fn build_rust_dependency_graph(parsed_files: &[&ParsedRustFile]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();

    for parsed in parsed_files {
        let module_name = parsed
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("unknown"));

        // Store the actual file path for this module
        graph.paths.insert(module_name.clone(), parsed.path.clone());

        // Ensure the module exists in the graph
        graph.get_or_create_node(&module_name);

        // Extract use statements
        let imports = extract_rust_imports(&parsed.ast);

        for import in imports {
            graph.add_dependency(&module_name, &import);
        }
    }

    graph
}

/// Extract imported crate/module names from a Rust file
fn extract_rust_imports(ast: &syn::File) -> Vec<String> {
    let mut imports = Vec::new();

    for item in &ast.items {
        if let Item::Use(use_item) = item {
            collect_use_paths(&use_item.tree, &mut imports);
        }
    }

    imports
}

/// Recursively collect crate names from use trees
fn collect_use_paths(tree: &syn::UseTree, imports: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(path) => {
            // Get the first segment (crate name)
            let crate_name = path.ident.to_string();
            
            // Skip common preludes and self/super/crate
            if !matches!(crate_name.as_str(), "self" | "super" | "crate") {
                imports.push(crate_name);
            }
            
            // Don't recurse - we only want top-level crate names
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
        syn::UseTree::Glob(_) => {
            // Can't determine specific imports from glob
        }
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
        assert!(imports.contains(&String::from("std")), "imports: {:?}", imports);
    }

    #[test]
    fn extracts_path_use() {
        let ast = parse_rust_code("use std::collections::HashMap;");
        let imports = extract_rust_imports(&ast);
        assert!(imports.contains(&String::from("std")), "imports: {:?}", imports);
    }

    #[test]
    fn extracts_multiple_uses() {
        let ast = parse_rust_code(
            r#"
use std::io;
use serde::Serialize;
use crate::module;
"#,
        );
        let imports = extract_rust_imports(&ast);
        assert!(imports.contains(&String::from("std")), "imports: {:?}", imports);
        assert!(imports.contains(&String::from("serde")), "imports: {:?}", imports);
        // crate:: is excluded
        assert!(!imports.contains(&String::from("crate")), "imports: {:?}", imports);
    }

    #[test]
    fn handles_grouped_uses() {
        let ast = parse_rust_code("use std::{io, collections::HashMap};");
        let imports = extract_rust_imports(&ast);
        assert!(imports.contains(&String::from("std")), "imports: {:?}", imports);
    }
}

