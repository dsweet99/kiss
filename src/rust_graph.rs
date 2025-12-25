
use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use syn::Item;

pub fn build_rust_dependency_graph(parsed_files: &[&ParsedRustFile]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();

    for parsed in parsed_files {
        let module_name = parsed
            .path
            .file_stem().map_or_else(|| String::from("unknown"), |s| s.to_string_lossy().into_owned());

        graph.paths.insert(module_name.clone(), parsed.path.clone());
        graph.get_or_create_node(&module_name);
        let imports = extract_rust_imports(&parsed.ast);

        for import in imports {
            graph.add_dependency(&module_name, &import);
        }
    }

    graph
}

fn extract_rust_imports(ast: &syn::File) -> Vec<String> {
    let mut imports = Vec::new();

    for item in &ast.items {
        if let Item::Use(use_item) = item {
            collect_use_paths(&use_item.tree, &mut imports);
        }
    }

    imports
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
        assert!(imports.contains(&String::from("std")), "imports: {imports:?}");
    }

    #[test]
    fn extracts_path_use() {
        let ast = parse_rust_code("use std::collections::HashMap;");
        let imports = extract_rust_imports(&ast);
        assert!(imports.contains(&String::from("std")), "imports: {imports:?}");
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
        assert!(imports.contains(&String::from("std")), "imports: {imports:?}");
        assert!(imports.contains(&String::from("serde")), "imports: {imports:?}");
        assert!(!imports.contains(&String::from("crate")), "crate:: should be excluded");
    }

    #[test]
    fn handles_grouped_uses() {
        let ast = parse_rust_code("use std::{io, collections::HashMap};");
        let imports = extract_rust_imports(&ast);
        assert!(imports.contains(&String::from("std")), "imports: {imports:?}");
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
}

