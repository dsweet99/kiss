use super::*;

fn parse_rust_code(code: &str) -> syn::File {
    syn::parse_file(code).expect("Failed to parse Rust code")
}

#[test]
fn include_stem_strips_rs_and_inc_extensions() {
    assert_eq!(
        crate::rust_include::include_stem_from_literal("path/foo.inc"),
        "foo"
    );
    assert_eq!(
        crate::rust_include::include_stem_from_literal("bar.rs"),
        "bar"
    );
}

#[test]
fn extracts_simple_use() {
    let ast = parse_rust_code("use std;");
    let imports = extract_rust_imports(&ast).use_roots;
    assert!(
        imports.contains(&String::from("std")),
        "imports: {imports:?}"
    );
}

#[test]
fn extracts_path_use() {
    let ast = parse_rust_code("use std::collections::HashMap;");
    let imports = extract_rust_imports(&ast).use_roots;
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
    let imports = extract_rust_imports(&ast).use_roots;
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
    let imports = extract_rust_imports(&ast).use_roots;
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
    let ast = parse_rust_code(
        r"
fn foo() {
    use std::fs;
    use serde::Serialize;
}
",
    );
    let imports = extract_rust_imports(&ast).use_roots;
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
    let imports = extract_rust_imports(&ast).use_roots;
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
    let imports = extract_rust_imports(&ast).use_roots;
    assert!(
        imports.contains(&String::from("tokio")),
        "inline module use not found: {imports:?}"
    );
}

#[test]
fn mod_decls_prefer_child_module_under_same_parent() {
    use std::io::Write;

    fn has_edge(g: &DependencyGraph, from: &str, to: &str) -> bool {
        let from_idx = *g.nodes.get(from).expect("from node");
        let to_idx = *g.nodes.get(to).expect("to node");
        g.graph.contains_edge(from_idx, to_idx)
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    for d in ["a", "b"] {
        std::fs::create_dir_all(src.join(d)).unwrap();
        std::fs::write(src.join(d).join("mod.rs"), "mod foo;").unwrap();
    }
    let mut fa = std::fs::File::create(src.join("a").join("foo.rs")).unwrap();
    let mut fb = std::fs::File::create(src.join("b").join("foo.rs")).unwrap();
    writeln!(fa, "pub fn a() {{}}").unwrap();
    writeln!(fb, "pub fn b() {{}}").unwrap();

    let a_mod_parsed = crate::rust_parsing::parse_rust_file(&src.join("a").join("mod.rs")).unwrap();
    let b_mod_parsed = crate::rust_parsing::parse_rust_file(&src.join("b").join("mod.rs")).unwrap();
    let a_foo_parsed = crate::rust_parsing::parse_rust_file(&src.join("a").join("foo.rs")).unwrap();
    let b_foo_parsed = crate::rust_parsing::parse_rust_file(&src.join("b").join("foo.rs")).unwrap();
    let refs: Vec<&crate::rust_parsing::ParsedRustFile> =
        vec![&a_mod_parsed, &b_mod_parsed, &a_foo_parsed, &b_foo_parsed];
    let g = build_rust_dependency_graph(&refs);

    assert!(has_edge(&g, "a", "a.foo"));
    assert!(!has_edge(&g, "a", "b.foo"));
    assert!(has_edge(&g, "b", "b.foo"));
    assert!(!has_edge(&g, "b", "a.foo"));
}

#[test]
fn test_qualified_rust_module_name() {
    assert_eq!(qualified_rust_module_name(Path::new("src/foo.rs")), "foo");
    assert_eq!(
        qualified_rust_module_name(Path::new("src/foo/bar.rs")),
        "foo.bar"
    );
    assert_eq!(
        qualified_rust_module_name(Path::new("src/foo/mod.rs")),
        "foo"
    );
    assert_eq!(qualified_rust_module_name(Path::new("utils.rs")), "utils");
    assert_eq!(
        qualified_rust_module_name(Path::new("tests/integration/helpers.rs")),
        "integration.helpers"
    );
}

#[test]
fn test_same_stem_different_dirs_no_collision() {
    use std::io::Write;

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

    assert_eq!(
        graph.nodes.len(),
        2,
        "Two files named utils.rs in different dirs should produce 2 graph nodes, got: {:?}",
        graph.nodes.keys().collect::<Vec<_>>()
    );
}

#[test]
fn qualify_child_module_respects_crate_roots() {
    assert_eq!(qualify_child_module("lib", "foo"), "foo");
    assert_eq!(qualify_child_module("parent", "child"), "parent.child");
}

#[test]
fn resolve_import_adds_edges_for_internal_and_bare_modules() {
    let mut graph = DependencyGraph::default();
    let internal: HashSet<String> = std::iter::once("foo".into()).collect();
    let bare: HashMap<String, Vec<String>> = HashMap::new();
    resolve_import("foo", "bar", &internal, &bare, &mut graph);
    let bar_idx = *graph.nodes.get("bar").expect("bar node");
    let foo_idx = *graph.nodes.get("foo").expect("foo node");
    assert!(graph.graph.contains_edge(bar_idx, foo_idx));

    let mut bare_map: HashMap<String, Vec<String>> = HashMap::new();
    bare_map.insert("alias".into(), vec!["other.mod".into()]);
    resolve_import("alias", "consumer", &HashSet::new(), &bare_map, &mut graph);
    let consumer_idx = *graph.nodes.get("consumer").expect("consumer node");
    let other_idx = *graph.nodes.get("other.mod").expect("other.mod node");
    assert!(graph.graph.contains_edge(consumer_idx, other_idx));
}

#[test]
fn resolve_import_skips_self_edges_from_bare_map() {
    let mut graph = DependencyGraph::default();
    let mut bare_map: HashMap<String, Vec<String>> = HashMap::new();
    bare_map.insert("alias".into(), vec!["consumer".into(), "other.mod".into()]);
    resolve_import("alias", "consumer", &HashSet::new(), &bare_map, &mut graph);
    let consumer_idx = *graph.nodes.get("consumer").expect("consumer node");
    let other_idx = *graph.nodes.get("other.mod").expect("other.mod node");
    assert!(graph.graph.contains_edge(consumer_idx, other_idx));
    assert_eq!(graph.graph.edge_count(), 1);
}

#[test]
fn extract_imports_from_block_and_expr_directly() {
    let ast = parse_rust_code("fn f() { if true { use std::io; } else { use core::fmt; } }");
    let syn::Item::Fn(func) = &ast.items[0] else {
        panic!("expected function item");
    };
    let mut block_roots = Vec::new();
    let mut block_mods = Vec::new();
    let mut block_includes = Vec::new();
    extract_imports_from_block(
        &func.block,
        &mut block_roots,
        &mut block_mods,
        &mut block_includes,
    );
    assert!(block_roots.contains(&"std".to_string()));

    if let Some(syn::Stmt::Expr(expr, _)) = func
        .block
        .stmts
        .iter()
        .find(|s| matches!(s, syn::Stmt::Expr(_, _)))
    {
        let mut expr_roots = Vec::new();
        let mut expr_mods = Vec::new();
        let mut expr_includes = Vec::new();
        extract_imports_from_expr(expr, &mut expr_roots, &mut expr_mods, &mut expr_includes);
        assert!(!expr_roots.is_empty() || !expr_mods.is_empty());
    }
}

#[test]
fn resolve_import_ignores_unknown_modules() {
    let mut graph = DependencyGraph::default();
    resolve_import(
        "missing",
        "module",
        &HashSet::new(),
        &HashMap::new(),
        &mut graph,
    );
    assert!(graph.nodes.is_empty());
}

#[test]
fn rust_imports_and_push_include_edges() {
    let _ = RustImports {
        use_roots: vec!["std".into()],
        mod_decls: vec!["child".into()],
        include_literals: vec!["child.rs".into()],
        use_spans: vec![],
        mod_spans: vec![],
        include_spans: vec![],
    };
    let ast = parse_rust_code("use std::io; mod child;");
    let mut use_roots = Vec::new();
    let mut mod_decls = Vec::new();
    let mut include_literals = Vec::new();
    extract_imports_from_items(
        &ast.items,
        &mut use_roots,
        &mut mod_decls,
        &mut include_literals,
    );
    assert!(!use_roots.is_empty());
    let mac: syn::Macro = syn::parse_quote!(include!("child.rs"));
    super::extract_imports::push_include_edges(&mac, &mut mod_decls, &mut include_literals);
    assert_eq!(include_literals, vec!["child.rs"]);
}

#[test]
fn mixed_file_keeps_dual_origins_on_same_endpoint() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "mod helper;\nmod mixed;\n").unwrap();
    std::fs::write(src.join("helper.rs"), "pub fn f() {}\n").unwrap();
    std::fs::write(
        src.join("mixed.rs"),
        "use helper;\n#[cfg(test)]\nmod tests {\n    use helper;\n}\n",
    )
    .unwrap();
    let paths = vec![
        src.join("lib.rs"),
        src.join("helper.rs"),
        src.join("mixed.rs"),
    ];
    let parsed: Vec<_> = paths
        .iter()
        .map(|path| crate::rust_parsing::parse_rust_file(path).unwrap())
        .collect();
    let roles = crate::code_roles::build_source_role_index(&[], &parsed, &[], &paths).unwrap();
    let refs: Vec<_> = parsed.iter().collect();
    let ctx = build_rust_context_graph(&refs, &roles);
    let mixed = qualified_rust_module_name(&src.join("mixed.rs"));
    let tests = format!("{mixed}::tests");
    assert!(ctx.production_view().imports(&mixed, "helper"));
    assert!(!ctx.production_view().imports(&tests, "helper"));
    assert!(ctx.test_view().imports(&tests, "helper"));
    assert!(ctx.test_importers_of("helper").contains(&tests));
    assert!(!ctx.test_importers_of("helper").contains(&mixed));
}

#[test]
fn include_in_inline_test_mod_keeps_inline_importer() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "mod mixed;\n").unwrap();
    std::fs::write(
        src.join("mixed.rs"),
        "#[cfg(test)]\nmod tests {\n    include!(\"frag.rs\");\n}\n",
    )
    .unwrap();
    std::fs::write(src.join("frag.rs"), "pub fn g() {}\n").unwrap();
    let paths = vec![
        src.join("lib.rs"),
        src.join("mixed.rs"),
        src.join("frag.rs"),
    ];
    let parsed: Vec<_> = paths
        .iter()
        .map(|path| crate::rust_parsing::parse_rust_file(path).unwrap())
        .collect();
    let roles = crate::code_roles::build_source_role_index(&[], &parsed, &[], &paths).unwrap();
    let refs: Vec<_> = parsed.iter().collect();
    let ctx = build_rust_context_graph(&refs, &roles);
    let mixed = qualified_rust_module_name(&src.join("mixed.rs"));
    let tests = format!("{mixed}::tests");
    let frag = qualified_rust_module_name(&src.join("frag.rs"));
    assert!(!ctx.production_view().imports(&mixed, &frag));
    assert!(ctx.test_view().imports(&tests, &frag));
    assert!(ctx.test_importers_of(&frag).contains(&tests));
    assert!(!ctx.test_importers_of(&frag).contains(&mixed));
}
