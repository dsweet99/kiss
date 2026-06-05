use super::*;
use crate::rust_parsing::parse_rust_file;
use crate::rust_test_refs::analyze_rust_test_refs;

#[test]
fn direct_weighted_paths_exercised() {
    let tmp = tempfile::TempDir::new().unwrap();
    let handlers = tmp.path().join("crate/handlers");
    std::fs::create_dir_all(&handlers).unwrap();
    std::fs::write(
        handlers.join("mod.rs"),
        "pub fn run(seed: u64) -> u64 { seed + 1 }\n",
    )
    .unwrap();
    let lib = tmp.path().join("lib.rs");
    std::fs::write(
        &lib,
        "pub mod handlers;\npub use handlers::run;\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn dispatch() { let _ = crate::handlers::run(0); }\n}\n",
    )
    .unwrap();
    let parsed_lib = parse_rust_file(&lib).unwrap();
    let parsed_handler = parse_rust_file(&handlers.join("mod.rs")).unwrap();
    let refs: Vec<_> = [&parsed_lib, &parsed_handler].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    assert!(weighted.get(&lib).copied().unwrap_or(0) > 0);
    assert!(weighted.get(&parsed_handler.path).copied().unwrap_or(0) > 0);
}

#[test]
fn locate_fn_finds_impl_method() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("worker.rs");
    std::fs::write(
        &src,
        "pub struct Worker;\nimpl Worker {\n    pub fn run(&self) -> u32 { 1 }\n}\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let def = analyze_rust_test_refs(&[&parsed], None)
        .definitions
        .into_iter()
        .find(|d| d.name == "run")
        .expect("run def");
    assert!(locate_fn(&parsed, &def).is_some());
}

#[test]
fn direct_private_weighted_helpers() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("worker.rs");
    std::fs::write(
        &src,
        r#"pub struct Worker;
impl Worker {
    pub fn new() -> Self { Worker }
    pub fn heavy_a(n: u64) -> u64 {
        let mut acc = n;
        for i in 0..20 { if i == n { acc += 1; } }
        acc
    }
    pub fn heavy_b(n: u64) -> u64 {
        let mut acc = n;
        for i in 0..20 { if i == n { acc += 2; } }
        acc
    }
}
#[cfg(test)]
mod tests {
    use super::Worker;
    #[test]
    fn only_new() { let _ = Worker::new(); }
}
"#,
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let unref_set: std::collections::HashSet<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();
    let def = analysis
        .definitions
        .iter()
        .find(|d| d.name == "heavy_a")
        .expect("heavy_a");
    let metrics = crate::rust_fn_metrics::RustFunctionMetrics {
        statements: 10,
        arguments: 1,
        max_indentation: 1,
        nested_function_depth: 0,
        returns: 1,
        branches: 5,
        local_variables: 2,
        bool_parameters: 0,
        attributes: 0,
        calls: 1,
    };
    let parsed_by_path = std::collections::HashMap::from([(parsed.path.clone(), &parsed)]);
    let _ = rs_module_import_surface_credit(&analysis, def, &metrics, &[], &parsed_by_path);
    let _ = rs_import_surface_credit(&analysis, def, &metrics, &[], &parsed_by_path);
    let _ = impl_type_covering_tests(&analysis, &unref_set, def);
    let _ = rs_import_surface_credit(&analysis, def, &metrics, &[], &parsed_by_path);
}

#[test]
fn locate_fn_finds_nested_mod_function() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("nested.rs");
    std::fs::write(
        &src,
        "mod inner { pub fn compute(n: i32) -> i32 { n + 1 } }\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let def = analyze_rust_test_refs(&[&parsed], None)
        .definitions
        .into_iter()
        .find(|d| d.name == "compute")
        .expect("compute");
    let located = locate_fn(&parsed, &def).expect("located");
    assert_eq!(located.block.stmts.len(), 1);
}

#[test]
fn direct_accumulate_and_locate_helpers() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    std::fs::write(
        &src,
        "pub fn used() -> i32 { 1 }\npub fn unused() -> i32 { 2 }\npub use used;\npub use unused;\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let mut by_file = std::collections::HashMap::new();
    let unref_set: std::collections::HashSet<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();
    accumulate_pub_use_export_mass(&parsed, &analysis, &unref_set, &mut by_file);
    assert!(by_file.contains_key(&src));
    let file: syn::File = syn::parse_str("use a::{b, c};").unwrap();
    if let syn::Item::Use(u) = &file.items[0] {
        let names = flatten_use_tree_names(&u.tree);
        assert_eq!(names, vec!["b", "c"]);
    }
}

#[test]
fn high_branch_locate_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("mix.rs");
    std::fs::write(
        &src,
        "mod inner { pub fn nested(n: i32) -> i32 { n + 1 } }\npub struct S;\nimpl S { pub fn m(&self) -> i32 { 1 } }\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let analysis = analyze_rust_test_refs(&[&parsed], None);
    for (i, def) in analysis.definitions.iter().enumerate() {
        if i % 2 == 0 {
            assert!(locate_fn(&parsed, def).is_some() || def.name == "S");
        } else {
            let _ = locate_in_item(&parsed.ast.items[0], def);
        }
    }
}
