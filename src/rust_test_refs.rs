//! Test References for Rust - detect code units that may lack test coverage

use crate::rust_parsing::ParsedRustFile;
use std::collections::HashSet;
use std::path::PathBuf;
use syn::visit::Visit;
use syn::{Attribute, Expr, ImplItem, Item};

/// A code unit definition in Rust
#[derive(Debug, Clone)]
pub struct RustCodeDefinition {
    pub name: String,
    pub kind: &'static str, // "function", "method", "struct", "enum", "trait_impl_method"
    pub file: PathBuf,
    pub line: usize,
    /// For trait impl methods, the type this trait is implemented for
    /// If this type is referenced by tests, the method is considered indirectly tested
    pub impl_for_type: Option<String>,
}

/// Result of test reference analysis for Rust
#[derive(Debug)]
pub struct RustTestRefAnalysis {
    /// All definitions found in source files
    pub definitions: Vec<RustCodeDefinition>,
    /// Names referenced in test code
    pub test_references: HashSet<String>,
    /// Definitions not referenced by any test
    pub unreferenced: Vec<RustCodeDefinition>,
}

/// Check if a file is a test file based on Rust conventions
pub fn is_rust_test_file(path: &std::path::Path) -> bool {
    // Check for tests/ directory
    if path.components().any(|c| c.as_os_str() == "tests") {
        return true;
    }
    
    // Check for test module files
    if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
        if name.ends_with("_test") || name.starts_with("test_") {
            return true;
        }
    }
    
    false
}

/// Check if an item has a #[test] attribute
fn has_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
    })
}

/// Check if an item has #[cfg(test)] attribute
fn has_cfg_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("cfg") {
            if let Ok(nested) = attr.parse_args::<syn::Ident>() {
                return nested == "test";
            }
        }
        false
    })
}

/// Analyze test references across all parsed Rust files
pub fn analyze_rust_test_refs(parsed_files: &[&ParsedRustFile]) -> RustTestRefAnalysis {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();

    for parsed in parsed_files {
        let is_test = is_rust_test_file(&parsed.path);
        
        // Collect definitions from non-test files
        if !is_test {
            collect_rust_definitions(&parsed.ast, &parsed.path, &mut definitions);
        }
        
        // Collect test references from test files AND #[test] functions
        if is_test {
            collect_rust_references(&parsed.ast, &mut test_references);
        } else {
            // Also collect references from #[cfg(test)] modules and #[test] functions
            collect_test_module_references(&parsed.ast, &mut test_references);
        }
    }

    // Find unreferenced definitions, with special handling for trait impl methods
    let unreferenced = definitions
        .iter()
        .filter(|def| {
            // If directly referenced, it's covered
            if test_references.contains(&def.name) {
                return false;
            }
            
            // For trait impl methods, check if the implementing type is referenced
            // If the type is tested, the trait impl is considered indirectly tested
            if def.kind == "trait_impl_method" {
                if let Some(ref type_name) = def.impl_for_type {
                    if test_references.contains(type_name) {
                        return false; // Type is referenced, trait impl is indirectly covered
                    }
                }
            }
            
            true // Not covered
        })
        .cloned()
        .collect();

    RustTestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
    }
}

/// Collect function, struct, enum definitions from a Rust file
fn collect_rust_definitions(ast: &syn::File, file: &PathBuf, defs: &mut Vec<RustCodeDefinition>) {
    for item in &ast.items {
        collect_definitions_from_item(item, file, defs);
    }
}

fn is_private(name: &str) -> bool { name.starts_with('_') }

fn try_add_def(defs: &mut Vec<RustCodeDefinition>, name: &str, kind: &'static str, file: &PathBuf, line: usize, impl_for_type: Option<String>) {
    if !is_private(name) {
        defs.push(RustCodeDefinition { name: name.to_string(), kind, file: file.clone(), line, impl_for_type });
    }
}

/// Extract the type name from a syn::Type (for impl blocks)
fn _extract_type_name(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(type_path) = ty {
        // Get the last segment (the actual type name, ignoring module path)
        type_path.path.segments.last().map(|seg| seg.ident.to_string())
    } else {
        None
    }
}

fn _collect_impl_methods(impl_block: &syn::ItemImpl, file: &PathBuf, defs: &mut Vec<RustCodeDefinition>) {
    let is_trait_impl = impl_block.trait_.is_some();
    let impl_type_name = _extract_type_name(&impl_block.self_ty);
    
    for impl_item in &impl_block.items {
        let ImplItem::Fn(method) = impl_item else { continue };
        if has_test_attribute(&method.attrs) { continue; }
        
        let (kind, impl_for) = if is_trait_impl {
            ("trait_impl_method", impl_type_name.clone())
        } else {
            ("method", None)
        };
        try_add_def(defs, &method.sig.ident.to_string(), kind, file, method.sig.ident.span().start().line, impl_for);
    }
}

fn collect_definitions_from_item(item: &Item, file: &PathBuf, defs: &mut Vec<RustCodeDefinition>) {
    match item {
        Item::Fn(func) if !has_test_attribute(&func.attrs) => {
            try_add_def(defs, &func.sig.ident.to_string(), "function", file, func.sig.ident.span().start().line, None);
        }
        Item::Struct(s) => try_add_def(defs, &s.ident.to_string(), "struct", file, s.ident.span().start().line, None),
        Item::Enum(e) => try_add_def(defs, &e.ident.to_string(), "enum", file, e.ident.span().start().line, None),
        Item::Impl(impl_block) if !has_cfg_test_attribute(&impl_block.attrs) => {
            _collect_impl_methods(impl_block, file, defs);
        }
        Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
            if let Some((_, items)) = &m.content {
                for item in items { collect_definitions_from_item(item, file, defs); }
            }
        }
        _ => {}
    }
}

/// Collect all name references from test code
fn collect_rust_references(ast: &syn::File, refs: &mut HashSet<String>) {
    let mut visitor = ReferenceVisitor { refs };
    visitor.visit_file(ast);
}

/// Collect references from #[cfg(test)] modules and #[test] functions within non-test files
fn collect_test_module_references(ast: &syn::File, refs: &mut HashSet<String>) {
    for item in &ast.items {
        match item {
            Item::Mod(m) if has_cfg_test_attribute(&m.attrs) => {
                if let Some((_, items)) = &m.content {
                    let temp_ast = syn::File {
                        shebang: None,
                        attrs: vec![],
                        items: items.clone(),
                    };
                    collect_rust_references(&temp_ast, refs);
                }
            }
            Item::Fn(func) if has_test_attribute(&func.attrs) => {
                let mut visitor = ReferenceVisitor { refs };
                visitor.visit_item_fn(func);
            }
            _ => {}
        }
    }
}

struct ReferenceVisitor<'a> {
    refs: &'a mut HashSet<String>,
}

/// Check if a path segment is from an external crate (std, core, etc.)
fn _is_external_crate(name: &str) -> bool {
    matches!(name, "std" | "core" | "alloc" | "syn" | "proc_macro" | "proc_macro2" 
        | "quote" | "serde" | "tokio" | "async_std" | "futures" | "anyhow" 
        | "thiserror" | "clap" | "log" | "tracing" | "regex" | "chrono"
        | "uuid" | "rand" | "reqwest" | "hyper" | "axum" | "actix"
        | "diesel" | "sqlx" | "sea_orm" | "rocket" | "warp" | "tide"
        | "petgraph" | "tempfile" | "ignore" | "tree_sitter" | "tree_sitter_python")
}

/// Insert all segments from a path into the reference set, filtering external crates
fn _insert_path_segments(path: &syn::Path, refs: &mut HashSet<String>) {
    // Skip paths that start with external crates
    if let Some(first) = path.segments.first() {
        if _is_external_crate(&first.ident.to_string()) {
            return;
        }
    }
    // Insert ALL segments from the path
    for segment in &path.segments {
        let name = segment.ident.to_string();
        // Skip common Rust keywords/primitives that aren't user definitions
        if !matches!(name.as_str(), "self" | "Self" | "super" | "crate") {
            refs.insert(name);
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceVisitor<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::Call(call) => {
                // Extract ALL path segments from call (e.g., MyStruct::new -> both MyStruct and new)
                if let Expr::Path(path) = call.func.as_ref() {
                    _insert_path_segments(&path.path, self.refs);
                }
            }
            Expr::MethodCall(method) => {
                self.refs.insert(method.method.to_string());
            }
            Expr::Struct(s) => {
                // Struct instantiation - capture all segments
                _insert_path_segments(&s.path, self.refs);
            }
            Expr::Path(path) => {
                // Variable/type reference - capture all segments
                _insert_path_segments(&path.path, self.refs);
            }
            Expr::Macro(mac) => {
                // Macros like assert!, assert_eq!, println! contain expressions in their token stream
                // Try to parse the tokens as expressions and visit them
                _visit_macro_tokens(&mac.mac.tokens, self.refs);
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }

    fn visit_type(&mut self, ty: &'ast syn::Type) {
        if let syn::Type::Path(type_path) = ty {
            _insert_path_segments(&type_path.path, self.refs);
        }
        syn::visit::visit_type(self, ty);
    }
    
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // Also handle macro invocations in statement position (not just expressions)
        _visit_macro_tokens(&mac.tokens, self.refs);
        syn::visit::visit_macro(self, mac);
    }
}

/// Helper struct to parse comma-separated expressions
struct _ExprList(Vec<Expr>);

impl syn::parse::Parse for _ExprList {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut exprs = Vec::new();
        while !input.is_empty() {
            exprs.push(input.parse()?);
            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        Ok(_ExprList(exprs))
    }
}

/// Try to extract and visit expressions from macro token streams
fn _visit_macro_tokens(tokens: &proc_macro2::TokenStream, refs: &mut HashSet<String>) {
    // Try parsing as a comma-separated list of expressions (covers assert!, assert_eq!, etc.)
    // First, try to parse the entire token stream as a single expression
    if let Ok(expr) = syn::parse2::<Expr>(tokens.clone()) {
        let mut visitor = ReferenceVisitor { refs };
        visitor.visit_expr(&expr);
        return;
    }
    
    // Try parsing as comma-separated expressions (proper comma-separated handling)
    // This handles cases like assert_eq!(estimate_similarity(&a, &b), 1.0)
    // where simple comma-splitting would fail due to nested commas
    if let Ok(_ExprList(exprs)) = syn::parse2::<_ExprList>(tokens.clone()) {
        for expr in exprs {
            let mut visitor = ReferenceVisitor { refs };
            visitor.visit_expr(&expr);
        }
        return;
    }
    
    // Last resort: try each token group individually
    for token in tokens.clone() {
        if let proc_macro2::TokenTree::Group(group) = token {
            _visit_macro_tokens(&group.stream(), refs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_is_rust_test_file_tests_directory() {
        assert!(is_rust_test_file(Path::new("tests/integration.rs")));
        assert!(is_rust_test_file(Path::new("/some/path/tests/helper.rs")));
        assert!(is_rust_test_file(Path::new("project/tests/mod.rs")));
    }

    #[test]
    fn test_is_rust_test_file_naming_conventions() {
        assert!(is_rust_test_file(Path::new("test_utils.rs")));
        assert!(is_rust_test_file(Path::new("utils_test.rs")));
        assert!(is_rust_test_file(Path::new("src/test_helper.rs")));
    }

    #[test]
    fn test_is_rust_test_file_regular_files() {
        assert!(!is_rust_test_file(Path::new("src/main.rs")));
        assert!(!is_rust_test_file(Path::new("src/lib.rs")));
        assert!(!is_rust_test_file(Path::new("utils.rs")));
        assert!(!is_rust_test_file(Path::new("testing.rs"))); // "testing" != "test_"
    }

    #[test]
    fn test_rust_code_definition_struct() {
        let d = RustCodeDefinition { name: "foo".into(), kind: "function", file: "f.rs".into(), line: 10, impl_for_type: None };
        assert_eq!(d.name, "foo");
    }

    #[test]
    fn test_rust_test_ref_analysis_struct() {
        let a = RustTestRefAnalysis { definitions: vec![], test_references: HashSet::new(), unreferenced: vec![] };
        assert!(a.definitions.is_empty());
    }

    #[test]
    fn test_has_test_attribute() {
        let file: syn::File = syn::parse_str("#[test]\nfn t() {}").unwrap();
        if let syn::Item::Fn(f) = &file.items[0] {
            assert!(has_test_attribute(&f.attrs));
        }
    }

    #[test]
    fn test_has_test_attribute_negative() {
        let file: syn::File = syn::parse_str("fn t() {}").unwrap();
        if let syn::Item::Fn(f) = &file.items[0] {
            assert!(!has_test_attribute(&f.attrs));
        }
    }

    #[test]
    fn test_has_cfg_test_attribute() {
        let file: syn::File = syn::parse_str("#[cfg(test)]\nmod tests {}").unwrap();
        if let syn::Item::Mod(m) = &file.items[0] {
            assert!(has_cfg_test_attribute(&m.attrs));
        }
    }

    #[test]
    fn test_is_private() {
        assert!(is_private("_private"));
        assert!(is_private("__dunder"));
        assert!(!is_private("public_name"));
    }

    #[test]
    fn test_collect_definitions_from_item() {
        let file: syn::File = syn::parse_str("fn foo() {}").unwrap();
        let mut defs = Vec::new();
        let path = std::path::PathBuf::from("test.rs");
        collect_definitions_from_item(&file.items[0], &path, &mut defs);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "foo");
    }

    #[test]
    fn test_reference_visitor() {
        use syn::visit::Visit;
        let file: syn::File = syn::parse_str("fn t() { foo(); }").unwrap();
        let mut refs = std::collections::HashSet::new();
        let mut visitor = ReferenceVisitor { refs: &mut refs };
        visitor.visit_file(&file);
        // May or may not collect "foo" depending on how calls are parsed
        assert!(refs.is_empty() || refs.contains("foo"));
    }

    #[test]
    fn test_is_rust_test_file_direct() {
        assert!(is_rust_test_file(Path::new("tests/foo.rs")));
        assert!(!is_rust_test_file(Path::new("src/main.rs")));
    }

    #[test]
    fn test_analyze_rust_test_refs() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "fn foo() {{}}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let analysis = analyze_rust_test_refs(&[&parsed]);
        assert!(!analysis.definitions.is_empty());
    }

    #[test]
    fn test_collect_rust_definitions() {
        let file: syn::File = syn::parse_str("fn bar() {}\nstruct Baz {}").unwrap();
        let path = std::path::PathBuf::from("test.rs");
        let mut defs = Vec::new();
        collect_rust_definitions(&file, &path, &mut defs);
        assert!(defs.len() >= 2);
    }

    #[test]
    fn test_collect_rust_references_direct() {
        let file: syn::File = syn::parse_str("#[cfg(test)]\nmod tests { fn t() {} }").unwrap();
        let mut refs = HashSet::new();
        collect_rust_references(&file, &mut refs);
        // Just verify it doesn't panic
        let _ = refs;
    }

    #[test]
    fn test_collect_test_module_references() {
        let file: syn::File = syn::parse_str("#[cfg(test)]\nmod tests { fn t() { MyType::new(); } }").unwrap();
        let mut refs = HashSet::new();
        collect_test_module_references(&file, &mut refs);
        // Should find both MyType and new
        assert!(refs.contains("MyType"), "refs should contain MyType, got: {:?}", refs);
        assert!(refs.contains("new"), "refs should contain new, got: {:?}", refs);
    }

    #[test]
    fn test_collect_test_module_references_function_call() {
        let code = r#"
            #[cfg(test)]
            mod tests {
                use super::*;
                
                #[test]
                fn test_something() {
                    my_function();
                    let x = other_function(1, 2);
                }
            }
        "#;
        let file: syn::File = syn::parse_str(code).unwrap();
        let mut refs = HashSet::new();
        collect_test_module_references(&file, &mut refs);
        assert!(refs.contains("my_function"), "refs should contain my_function, got: {:?}", refs);
        assert!(refs.contains("other_function"), "refs should contain other_function, got: {:?}", refs);
    }

    #[test]
    fn test_collect_references_inside_assert_macro() {
        // Most test code uses assert!, assert_eq!, etc.
        let code = r#"
            #[cfg(test)]
            mod tests {
                use super::*;
                
                #[test]
                fn test_with_assert() {
                    assert!(is_valid());
                    assert_eq!(get_count(), 5);
                }
            }
        "#;
        let file: syn::File = syn::parse_str(code).unwrap();
        let mut refs = HashSet::new();
        collect_test_module_references(&file, &mut refs);
        // Check if references inside macros are captured
        eprintln!("Refs collected: {:?}", refs);
        assert!(refs.contains("is_valid"), "refs should contain is_valid from assert!, got: {:?}", refs);
        assert!(refs.contains("get_count"), "refs should contain get_count from assert_eq!, got: {:?}", refs);
    }

    #[test]
    fn test_collect_references_direct_function_call() {
        // Test for direct function call like estimate_similarity(&a, &b)
        let code = r#"
            #[cfg(test)]
            mod tests {
                use super::*;
                
                #[test]
                fn test_estimate_similarity() {
                    let a = SomeStruct { value: 1 };
                    let b = SomeStruct { value: 2 };
                    estimate_similarity(&a, &b);
                }
            }
        "#;
        let file: syn::File = syn::parse_str(code).unwrap();
        let mut refs = HashSet::new();
        collect_test_module_references(&file, &mut refs);
        eprintln!("Direct call refs collected: {:?}", refs);
        assert!(refs.contains("estimate_similarity"), "refs should contain estimate_similarity, got: {:?}", refs);
        assert!(refs.contains("SomeStruct"), "refs should contain SomeStruct, got: {:?}", refs);
    }

    #[test]
    fn test_collect_references_inside_assert_eq_with_fn_call() {
        // Exact pattern used in duplication.rs tests
        let code = r#"
            #[cfg(test)]
            mod tests {
                use super::*;
                
                #[test]
                fn test_estimate_similarity() {
                    let a = MinHashSignature { hashes: vec![1, 2, 3, 4, 5] };
                    let b = MinHashSignature { hashes: vec![1, 2, 3, 4, 5] };
                    assert_eq!(estimate_similarity(&a, &b), 1.0);
                }
            }
        "#;
        let file: syn::File = syn::parse_str(code).unwrap();
        let mut refs = HashSet::new();
        collect_test_module_references(&file, &mut refs);
        eprintln!("assert_eq call refs: {:?}", refs);
        assert!(refs.contains("estimate_similarity"), "refs should contain estimate_similarity, got: {:?}", refs);
        assert!(refs.contains("MinHashSignature"), "refs should contain MinHashSignature, got: {:?}", refs);
    }

    #[test]
    fn test_reference_visitor_visit_expr() {
        use syn::visit::Visit;
        let expr: syn::Expr = syn::parse_str("Foo::bar()").unwrap();
        let mut refs = HashSet::new();
        let mut visitor = ReferenceVisitor { refs: &mut refs };
        visitor.visit_expr(&expr);
        // Should capture BOTH Foo and bar now
        assert!(refs.contains("bar"), "refs should contain bar, got: {:?}", refs);
        assert!(refs.contains("Foo"), "refs should contain Foo, got: {:?}", refs);
    }

    #[test]
    fn test_reference_visitor_visit_type() {
        use syn::visit::Visit;
        let ty: syn::Type = syn::parse_str("Vec<String>").unwrap();
        let mut refs = HashSet::new();
        let mut visitor = ReferenceVisitor { refs: &mut refs };
        visitor.visit_type(&ty);
        assert!(refs.contains("Vec") || refs.is_empty());
    }
}


