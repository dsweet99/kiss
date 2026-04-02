use syn::visit::Visit;
use syn::{Block, Expr, Pat, Stmt};

use crate::rust_parsing::ParsedRustFile;

#[derive(Debug, Default)]
pub struct RustFunctionMetrics {
    pub statements: usize,
    pub arguments: usize,
    pub max_indentation: usize,
    pub nested_function_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub bool_parameters: usize,
    pub attributes: usize,
    pub calls: usize,
}

#[derive(Debug, Default)]
pub struct RustTypeMetrics {
    pub methods: usize,
}

#[derive(Debug, Default)]
pub struct RustFileMetrics {
    pub statements: usize,
    pub interface_types: usize,
    pub concrete_types: usize,
    pub imports: usize,
    pub functions: usize,
}

#[must_use]
pub fn compute_rust_file_metrics(parsed: &ParsedRustFile) -> RustFileMetrics {
    let mut metrics = RustFileMetrics::default();
    accumulate_rust_file_metrics_from_items(&parsed.ast.items, &mut metrics);
    metrics
}

fn accumulate_rust_file_metrics_from_items(items: &[syn::Item], out: &mut RustFileMetrics) {
    for item in items {
        match item {
            syn::Item::Trait(_) => out.interface_types += 1,
            syn::Item::Struct(_) | syn::Item::Enum(_) | syn::Item::Union(_) => {
                out.concrete_types += 1;
            }
            syn::Item::Use(u) if matches!(u.vis, syn::Visibility::Inherited) => {
                out.imports += count_use_names(&u.tree);
            }
            syn::Item::Fn(f) => {
                out.functions += 1;
                let mut visitor = FunctionMetricsVisitor::default();
                visitor.visit_block(&f.block);
                out.statements += visitor.statements;
            }
            syn::Item::Impl(imp) => {
                for impl_item in &imp.items {
                    if let syn::ImplItem::Fn(f) = impl_item {
                        out.functions += 1;
                        let mut visitor = FunctionMetricsVisitor::default();
                        visitor.visit_block(&f.block);
                        out.statements += visitor.statements;
                    }
                }
            }
            syn::Item::Mod(m) => {
                if !is_cfg_test_mod(m)
                    && let Some((_, nested_items)) = &m.content
                {
                    accumulate_rust_file_metrics_from_items(nested_items, out);
                }
            }
            _ => {}
        }
    }
}

/// Returns true if the module has a `#[cfg(test)]` or similar attribute indicating test code.
///
/// Handles compound expressions like `#[cfg(all(test, ...))]` and negations
/// like `#[cfg(not(test))]` (returns false) and `#[cfg(not(not(test)))]` (returns true).
pub fn is_cfg_test_mod(m: &syn::ItemMod) -> bool {
    m.attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<proc_macro2::TokenStream>()
            .map(|ts| contains_test_ident(ts, false))
            .unwrap_or(false)
    })
}

fn contains_test_ident(tokens: proc_macro2::TokenStream, negated: bool) -> bool {
    use proc_macro2::TokenTree;
    let mut iter = tokens.into_iter();
    while let Some(tt) = iter.next() {
        match &tt {
            TokenTree::Ident(id) if *id == "test" => {
                // If we're inside an odd number of not() wrappers, this is NOT test code
                if !negated {
                    return true;
                }
            }
            TokenTree::Ident(id) if *id == "not" => {
                // Recurse into not() with flipped negation
                if let Some(TokenTree::Group(g)) = iter.next()
                    && contains_test_ident(g.stream(), !negated)
                {
                    return true;
                }
            }
            TokenTree::Ident(id) if *id == "all" || *id == "any" => {
                // Recurse into all/any groups, preserving current negation
                if let Some(TokenTree::Group(g)) = iter.next()
                    && contains_test_ident(g.stream(), negated)
                {
                    return true;
                }
            }
            TokenTree::Group(g) => {
                if contains_test_ident(g.stream(), negated) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Count attributes excluding doc comments (`#[doc = "..."]` / `///` lowered to `doc`).
///
/// Matches `kiss check` / `annotations_per_function` so stats and detailed output use the same rule.
#[must_use]
pub fn count_non_doc_attrs(attrs: &[syn::Attribute]) -> usize {
    attrs.iter().filter(|a| !a.path().is_ident("doc")).count()
}

/// Count the number of individual names imported by a `use` tree.
/// `use foo::bar;` → 1, `use foo::{bar, baz};` → 2, `use foo::*;` → 1 (glob counts as 1).
fn count_use_names(tree: &syn::UseTree) -> usize {
    match tree {
        syn::UseTree::Path(p) => count_use_names(&p.tree),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => 1,
        syn::UseTree::Group(g) => g.items.iter().map(count_use_names).sum(),
    }
}

// Allow: metrics are computed incrementally from different sources (args, visitor, attr_count)
#[allow(clippy::field_reassign_with_default)]
pub fn compute_rust_function_metrics(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    block: &Block,
    attr_count: usize,
) -> RustFunctionMetrics {
    let mut metrics = RustFunctionMetrics::default();

    let non_self_args: Vec<_> = inputs
        .iter()
        .filter(|arg| !matches!(arg, syn::FnArg::Receiver(_)))
        .collect();
    metrics.arguments = non_self_args.len();
    metrics.bool_parameters = non_self_args
        .iter()
        .filter(|arg| is_bool_param(arg))
        .count();
    metrics.attributes = attr_count;

    let mut visitor = FunctionMetricsVisitor::default();
    visitor.visit_block(block);

    metrics.statements = visitor.statements;
    metrics.max_indentation = visitor.max_depth;
    metrics.returns = visitor.returns;
    metrics.branches = visitor.branches;
    metrics.local_variables = visitor.local_variables;
    metrics.nested_function_depth = visitor.max_closure_depth;
    metrics.calls = visitor.calls;

    metrics
}

fn is_bool_param(arg: &syn::FnArg) -> bool {
    matches!(arg, syn::FnArg::Typed(pt) if matches!(&*pt.ty, syn::Type::Path(tp) if tp.path.is_ident("bool")))
}

#[derive(Default)]
pub struct FunctionMetricsVisitor {
    pub statements: usize,
    pub max_depth: usize,
    pub current_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub max_closure_depth: usize,
    pub current_closure_depth: usize,
    pub calls: usize,
}

impl FunctionMetricsVisitor {
    pub fn enter_block(&mut self) {
        self.current_depth += 1;
        self.max_depth = self.max_depth.max(self.current_depth);
    }

    pub const fn exit_block(&mut self) {
        self.current_depth -= 1;
    }

    pub fn count_pattern_bindings(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(_) => self.local_variables += 1,
            Pat::Type(typed) => self.count_pattern_bindings(&typed.pat),
            Pat::Tuple(tuple) => {
                for elem in &tuple.elems {
                    self.count_pattern_bindings(elem);
                }
            }
            Pat::TupleStruct(ts) => {
                for elem in &ts.elems {
                    self.count_pattern_bindings(elem);
                }
            }
            Pat::Struct(s) => {
                for field in &s.fields {
                    self.count_pattern_bindings(&field.pat);
                }
            }
            _ => {}
        }
    }
}

impl<'ast> Visit<'ast> for FunctionMetricsVisitor {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        // Statement definition: statements exclude imports and signatures.
        // Skip use statements inside function bodies
        let is_use_item = matches!(stmt, Stmt::Item(syn::Item::Use(_)));
        // Skip inner fn items: they are separate scopes whose body metrics
        // should not be attributed to the enclosing function.
        let is_inner_fn = matches!(stmt, Stmt::Item(syn::Item::Fn(_)));
        if !is_use_item {
            self.statements += 1;
        }
        if let Stmt::Local(local) = stmt {
            self.count_pattern_bindings(&local.pat);
        }
        if !is_inner_fn {
            syn::visit::visit_stmt(self, stmt);
        }
    }

    // Note: Rust match arms are NOT counted as branches (unlike Python case clauses).
    // Rust match is exhaustive pattern matching; Python match/case is optional branching.
    // This preserves semantic consistency: we count optional code paths, not exhaustive coverage.

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::If(_) => {
                self.branches += 1;
                self.enter_block();
            }
            Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => {
                self.enter_block();
            }
            Expr::Return(_) => {
                self.returns += 1;
            }
            Expr::Closure(_) => {
                self.current_closure_depth += 1;
                self.max_closure_depth = self.max_closure_depth.max(self.current_closure_depth);
            }
            Expr::Call(_) | Expr::MethodCall(_) => {
                self.calls += 1;
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
        match expr {
            Expr::If(_) | Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => {
                self.exit_block();
            }
            Expr::Closure(_) => self.current_closure_depth -= 1,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::visit::Visit;

    fn parse_fn(
        code: &str,
    ) -> (
        syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
        syn::Block,
    ) {
        let f: syn::File = syn::parse_str(code).unwrap();
        if let syn::Item::Fn(func) = &f.items[0] {
            (func.sig.inputs.clone(), (*func.block).clone())
        } else {
            panic!("Expected function")
        }
    }

    #[test]
    fn test_function_metrics() {
        let (i1, b1) = parse_fn("fn foo(a: i32, b: String, c: bool) {}");
        let m1 = compute_rust_function_metrics(&i1, &b1, 0);
        assert_eq!(m1.arguments, 3);
        assert_eq!(m1.bool_parameters, 1);

        let (i2, b2) = parse_fn(r#"fn f() { let x=1; let y=2; println!("{}",x+y); }"#);
        assert!(compute_rust_function_metrics(&i2, &b2, 0).statements >= 3);

        let (i3, b3) = parse_fn("fn f(x: i32) { if x>0 {} else if x<0 {} }");
        assert!(compute_rust_function_metrics(&i3, &b3, 0).branches >= 2);

        let (i4, b4) = parse_fn("fn f() { let a=1; let b=2; let (c,d)=(3,4); }");
        assert_eq!(
            compute_rust_function_metrics(&i4, &b4, 0).local_variables,
            4
        );

        let (i5, b5) = parse_fn("fn f() {}");
        assert_eq!(compute_rust_function_metrics(&i5, &b5, 3).attributes, 3);
    }

    #[test]
    fn test_visitor() {
        let mut v = FunctionMetricsVisitor::default();
        v.enter_block();
        assert_eq!(v.current_depth, 1);
        v.exit_block();

        let f: syn::File = syn::parse_str("fn f() { let x=1; let y=2; }").unwrap();
        if let syn::Item::Fn(func) = &f.items[0] {
            for s in &func.block.stmts {
                v.visit_stmt(s);
            }
        }
        assert!(v.statements >= 2);

        let e: syn::Expr = syn::parse_str("if true { 1 } else { 2 }").unwrap();
        let mut v2 = FunctionMetricsVisitor::default();
        v2.visit_expr(&e);
        assert!(v2.branches >= 1);

        let f2: syn::File = syn::parse_str("fn f() { let (a,b,c)=(1,2,3); }").unwrap();
        if let syn::Item::Fn(func) = &f2.items[0]
            && let syn::Stmt::Local(l) = &func.block.stmts[0]
        {
            let mut v3 = FunctionMetricsVisitor::default();
            v3.count_pattern_bindings(&l.pat);
            assert_eq!(v3.local_variables, 3);
        }
    }

    #[test]
    fn test_is_bool_param() {
        let f: syn::File = syn::parse_str("fn foo(a: bool, b: i32) {}").unwrap();
        if let syn::Item::Fn(func) = &f.items[0] {
            assert!(is_bool_param(&func.sig.inputs[0]));
            assert!(!is_bool_param(&func.sig.inputs[1]));
        }
    }

    // === Bug-hunting tests ===

    #[test]
    fn test_inner_fn_statements_not_counted_in_outer() {
        // Inner named functions are separate scopes. Their body statements should NOT
        // be counted in the outer function's statement count (matching Python behavior).
        let (inputs, block) =
            parse_fn("fn outer() { let x = 1; fn inner() { let y = 2; let z = 3; } }");
        let m = compute_rust_function_metrics(&inputs, &block, 0);
        // Expected: 2 statements (let x + fn inner as an item)
        // Bug: recursion counts inner's body too → 4
        assert_eq!(
            m.statements, 2,
            "Inner fn body statements should not count in outer fn (got {})",
            m.statements
        );
    }

    #[test]
    fn test_inner_fn_locals_not_counted_in_outer() {
        // Inner fn's local variables should not be attributed to the outer function.
        let (inputs, block) =
            parse_fn("fn outer() { let a = 1; fn inner() { let b = 2; let c = 3; } }");
        let m = compute_rust_function_metrics(&inputs, &block, 0);
        assert_eq!(
            m.local_variables, 1,
            "Inner fn locals should not count in outer fn (got {})",
            m.local_variables
        );
    }

    #[test]
    fn test_inner_fn_branches_not_counted_in_outer() {
        // Branches inside inner functions should not inflate outer function's branch count.
        let (inputs, block) =
            parse_fn("fn outer() { fn inner(x: i32) { if x > 0 {} if x < 0 {} } }");
        let m = compute_rust_function_metrics(&inputs, &block, 0);
        assert_eq!(
            m.branches, 0,
            "Inner fn branches should not count in outer fn (got {})",
            m.branches
        );
    }

    #[test]
    fn test_structs() {
        let _ = RustFunctionMetrics {
            statements: 1,
            arguments: 2,
            max_indentation: 3,
            returns: 4,
            branches: 5,
            local_variables: 6,
            nested_function_depth: 8,
            bool_parameters: 0,
            attributes: 0,
            calls: 2,
        };
        let _ = (
            RustTypeMetrics { methods: 5 },
            RustFileMetrics {
                statements: 100,
                interface_types: 1,
                concrete_types: 2,
                imports: 5,
                functions: 10,
            },
        );
    }

    #[test]
    fn test_file_metrics() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "use std::io;\nfn foo() {{ let x = 1; }}\ntrait T {{ fn x(&self) {{}} }}\nstruct A {{}}\nstruct B {{}}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let m = compute_rust_file_metrics(&parsed);
        assert!(
            m.statements >= 1 && m.interface_types == 1 && m.concrete_types == 2 && m.imports == 1
        );
    }

    #[test]
    fn test_use_statements_in_function_not_counted() {
        // Statement definition: any statement within a function body that is not an import or signature.
        // use statements inside function bodies should NOT be counted as statements
        let (_, b) = parse_fn("fn f() { use std::io::Write; let x = 1; println!(\"{}\", x); }");
        let m = compute_rust_function_metrics(&syn::punctuated::Punctuated::new(), &b, 0);
        // Should be 2 statements (let + println), not 3 (use + let + println)
        assert_eq!(
            m.statements, 2,
            "use statements inside functions should not be counted"
        );
    }

    #[test]
    fn test_count_use_names() {
        use std::io::Write;

        // Single name: `use foo::bar;`
        let u: syn::ItemUse = syn::parse_str("use foo::bar;").unwrap();
        assert_eq!(count_use_names(&u.tree), 1);

        // Grouped names: `use foo::{bar, baz};`
        let u2: syn::ItemUse = syn::parse_str("use foo::{bar, baz};").unwrap();
        assert_eq!(count_use_names(&u2.tree), 2);

        // Glob: `use foo::*;`
        let u3: syn::ItemUse = syn::parse_str("use foo::*;").unwrap();
        assert_eq!(count_use_names(&u3.tree), 1);

        // Rename: `use foo::bar as b;`
        let u4: syn::ItemUse = syn::parse_str("use foo::bar as b;").unwrap();
        assert_eq!(count_use_names(&u4.tree), 1);

        // Nested groups: `use foo::{bar, baz::{qux, quux}};`
        let u5: syn::ItemUse = syn::parse_str("use foo::{bar, baz::{qux, quux}};").unwrap();
        assert_eq!(count_use_names(&u5.tree), 3);

        // File-level counting: use items count imported names
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(
            tmp,
            "use std::io::{{Read, Write}};\nuse std::path::Path;\nfn main() {{}}"
        )
        .unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let m = compute_rust_file_metrics(&parsed);
        assert_eq!(
            m.imports, 3,
            "should count 3 imported names: Read, Write, Path"
        );
    }

    #[test]
    fn test_count_non_doc_attrs_excludes_doc() {
        let f: syn::File = syn::parse_str(r#"#[doc = "help"] #[inline] fn f() {}"#).unwrap();
        if let syn::Item::Fn(ff) = &f.items[0] {
            assert_eq!(count_non_doc_attrs(&ff.attrs), 1);
            let m = compute_rust_function_metrics(
                &ff.sig.inputs,
                &ff.block,
                count_non_doc_attrs(&ff.attrs),
            );
            assert_eq!(m.attributes, 1);
        } else {
            panic!("expected fn");
        }
    }

    #[test]
    fn test_file_metrics_nested_mod() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(
            tmp,
            r"
fn top_level() {{ let x = 1; }}
mod inner {{
    fn nested_fn() {{ let y = 2; let z = 3; }}
    struct InnerStruct {{}}
    trait InnerTrait {{}}
    impl InnerStruct {{
        fn method(&self) {{ let w = 4; }}
    }}
}}
"
        )
        .unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let m = compute_rust_file_metrics(&parsed);
        assert_eq!(m.functions, 3, "should count top_level + nested_fn + method");
        assert_eq!(m.statements, 4, "should count all statements in all fns");
        assert_eq!(m.concrete_types, 1, "should count InnerStruct");
        assert_eq!(m.interface_types, 1, "should count InnerTrait");
    }

    #[test]
    fn test_cfg_test_mod_compound_expression_skipped() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(
            tmp,
            r#"
fn production_fn() {{ let x = 1; }}

#[cfg(test)]
mod simple_test {{
    fn simple_test_fn() {{ let y = 2; }}
}}

#[cfg(all(test, feature = "expensive_tests"))]
mod compound_test {{
    fn compound_test_fn() {{ let z = 3; }}
}}
"#
        )
        .unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let m = compute_rust_file_metrics(&parsed);
        // Both test modules should be skipped - only production_fn should be counted
        assert_eq!(
            m.functions, 1,
            "should only count production_fn, not test fns (simple or compound cfg)"
        );
        assert_eq!(
            m.statements, 1,
            "should only count statements in production_fn"
        );
    }

    #[test]
    fn test_cfg_not_test_mod_included_in_metrics() {
        // BUG TEST: #[cfg(not(test))] means "production code", NOT test code.
        // It should be INCLUDED in file metrics, not skipped.
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(
            tmp,
            r"
fn always_fn() {{ let a = 1; }}

#[cfg(not(test))]
mod production_only {{
    fn prod_fn() {{ let b = 2; }}
}}

#[cfg(test)]
mod tests {{
    fn test_fn() {{ let c = 3; }}
}}
"
        )
        .unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let m = compute_rust_file_metrics(&parsed);
        // Should count always_fn + prod_fn = 2 functions, not just always_fn = 1
        // The tests module should be skipped, but not(test) module should be included
        assert_eq!(
            m.functions, 2,
            "cfg(not(test)) is production code and should be counted (got {})",
            m.functions
        );
        assert_eq!(
            m.statements, 2,
            "cfg(not(test)) statements should be counted (got {})",
            m.statements
        );
    }

    #[test]
    fn test_is_cfg_test_mod_semantics() {
        use syn::{parse_str, Item};

        // Test that is_cfg_test_mod correctly identifies test vs production modules
        let cases = [
            (r"#[cfg(test)] mod m {}", "cfg(test)", true),
            (r#"#[cfg(all(test, feature = "foo"))] mod m {}"#, "cfg(all(test,...))", true),
            (r"#[cfg(any(test, windows))] mod m {}", "cfg(any(test,...))", true),
            (r"#[cfg(not(test))] mod m {}", "cfg(not(test)) = PRODUCTION", false),
            (r#"#[cfg(feature = "foo")] mod m {}"#, "cfg(feature) = PRODUCTION", false),
            (r"mod m {}", "no cfg = PRODUCTION", false),
        ];

        for (code, label, expected) in cases {
            let item: Item = parse_str(code).unwrap();
            if let Item::Mod(m) = item {
                let result = is_cfg_test_mod(&m);
                println!("{label}: is_cfg_test_mod = {result}, expected = {expected}");
                assert_eq!(result, expected, "mismatch for {label}");
            }
        }
    }

    #[test]
    fn test_double_negation_not_not_test_is_test_code() {
        // BUG: not(not(test)) is logically equivalent to test, so it IS test-only code.
        // It should be SKIPPED in file metrics, not included.
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(
            tmp,
            r"
fn production_fn() {{ let a = 1; }}

#[cfg(not(not(test)))]
mod double_negation_test {{
    fn double_neg_fn() {{ let b = 2; }}
}}

#[cfg(test)]
mod tests {{
    fn test_fn() {{ let c = 3; }}
}}
"
        )
        .unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let m = compute_rust_file_metrics(&parsed);
        // not(not(test)) == test, so should only count production_fn (1 function, 1 statement)
        assert_eq!(
            m.functions, 1,
            "not(not(test)) is test code and should be skipped (got {} functions)",
            m.functions
        );
        assert_eq!(
            m.statements, 1,
            "not(not(test)) statements should not be counted (got {})",
            m.statements
        );
    }
}
