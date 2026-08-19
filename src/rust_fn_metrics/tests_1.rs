use super::*;
use std::io::Write;
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

    let (i_match, b_match) = parse_fn("fn f(x: u32) -> u32 { match x { 0 => 1, 1 => 2, _ => 0 } }");
    assert_eq!(
        compute_rust_function_metrics(&i_match, &b_match, 0).branches,
        3,
        "match arms should count as branches"
    );

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

    let mut v2 = FunctionMetricsVisitor::default();
    for code in [
        "if true { 1 } else { 2 }",
        "match 0 { 0 => 1, _ => 2 }",
        "while true { break; }",
        "for _ in 0..1 {}",
        "loop { break; }",
        "return 1",
        "|| 1",
        "foo(1)",
        "x.foo()",
    ] {
        let e: syn::Expr = syn::parse_str(code).unwrap();
        v2.visit_expr(&e);
    }
    assert!(v2.branches >= 1);
    assert!(v2.returns >= 1);
    assert!(v2.calls >= 2);

    let mut v_enter = FunctionMetricsVisitor::default();
    for code in [
        "if true { 1 }",
        "match 0 { _ => 1 }",
        "while true { break; }",
        "for _ in 0..1 {}",
        "loop { break; }",
        "return 1",
        "|| 1",
        "foo(1)",
        "x.foo()",
        "1 + 2",
    ] {
        let e: syn::Expr = syn::parse_str(code).unwrap();
        v_enter.on_enter_expr(&e);
        v_enter.on_exit_expr(&e);
    }

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



#[test]
fn test_inner_fn_statements_not_counted_in_outer() {


    let (inputs, block) =
        parse_fn("fn outer() { let x = 1; fn inner() { let y = 2; let z = 3; } }");
    let m = compute_rust_function_metrics(&inputs, &block, 0);


    assert_eq!(
        m.statements, 2,
        "Inner fn body statements should not count in outer fn (got {})",
        m.statements
    );
}

#[test]
fn test_inner_fn_locals_not_counted_in_outer() {

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

    let (inputs, block) = parse_fn("fn outer() { fn inner(x: i32) { if x > 0 {} if x < 0 {} } }");
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
    assert!(m.statements >= 1 && m.interface_types == 1 && m.concrete_types == 2 && m.imports == 1);
}

#[test]
fn test_use_statements_in_function_not_counted() {


    let (_, b) = parse_fn("fn f() { use std::io::Write; let x = 1; println!(\"{}\", x); }");
    let m = compute_rust_function_metrics(&syn::punctuated::Punctuated::new(), &b, 0);

    assert_eq!(
        m.statements, 2,
        "use statements inside functions should not be counted"
    );
}

#[test]
fn test_count_use_names() {
    use std::io::Write;


    let u: syn::ItemUse = syn::parse_str("use foo::bar;").unwrap();
    assert_eq!(count_use_names(&u.tree), 1);


    let u2: syn::ItemUse = syn::parse_str("use foo::{bar, baz};").unwrap();
    assert_eq!(count_use_names(&u2.tree), 2);


    let u3: syn::ItemUse = syn::parse_str("use foo::*;").unwrap();
    assert_eq!(count_use_names(&u3.tree), 1);


    let u4: syn::ItemUse = syn::parse_str("use foo::bar as b;").unwrap();
    assert_eq!(count_use_names(&u4.tree), 1);


    let u5: syn::ItemUse = syn::parse_str("use foo::{bar, baz::{qux, quux}};").unwrap();
    assert_eq!(count_use_names(&u5.tree), 3);


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
fn cfg_test_detection_handles_nested_boolean_forms() {
    let positive: syn::ItemMod =
        syn::parse_str("#[cfg(any(feature = \"x\", all(test)))] mod m {}").unwrap();
    assert!(is_cfg_test_mod(&positive));

    let negative: syn::ItemMod =
        syn::parse_str("#[cfg(all(not(test), feature = \"x\"))] mod m {}").unwrap();
    assert!(!is_cfg_test_mod(&negative));

    let double_negative: syn::ItemMod = syn::parse_str("#[cfg(not(not(test)))] mod m {}").unwrap();
    assert!(is_cfg_test_mod(&double_negative));
}

#[test]
fn function_metrics_count_struct_tuple_and_typed_pattern_bindings() {
    let func: syn::ItemFn = syn::parse_str(
        "fn f(value: bool) { let Point { x, y } = p; let Pair(a, b) = q; let z: i32 = 1; if value { call(); } }",
    )
    .unwrap();

    let metrics = compute_rust_function_metrics(
        &func.sig.inputs,
        &func.block,
        count_non_doc_attrs(&func.attrs),
    );

    assert_eq!(metrics.arguments, 1);
    assert_eq!(metrics.bool_parameters, 1);
    assert!(metrics.local_variables >= 5);
    assert_eq!(metrics.branches, 1);
    assert_eq!(metrics.calls, 1);
}

#[test]
fn function_metrics_count_closures_calls_and_skip_inner_function_bodies() {
    let (inputs, block) = parse_fn(
        r#"
        fn outer(flag: bool) {
            use std::fmt;
            fn inner() {
                let hidden = 1;
                return;
            }
            let f = || || helper();
            if flag {
                f()();
            }
            match 1 {
                0 => return,
                1 => helper(),
                _ => value.method(),
            }
        }
        "#,
    );
    let metrics = compute_rust_function_metrics(&inputs, &block, 0);

    assert_eq!(metrics.bool_parameters, 1);
    assert_eq!(metrics.returns, 1);
    assert_eq!(metrics.branches, 4);
    assert_eq!(metrics.nested_function_depth, 2);
    assert!(metrics.calls >= 4);
    assert_eq!(metrics.local_variables, 1);
}

#[test]
fn file_metrics_count_traits_concrete_types_and_nested_non_test_modules() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        "trait T {{}}\nstruct S;\nenum E {{ A }}\nunion U {{ a: u8 }}\nmod nested {{ pub fn f() {{ let x = 1; }} }}\n#[cfg(test)] mod tests {{ fn hidden() {{ let y = 1; }} }}",
    )
    .unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let metrics = compute_rust_file_metrics(&parsed);

    assert_eq!(metrics.interface_types, 1);
    assert_eq!(metrics.concrete_types, 3);
    assert_eq!(metrics.functions, 1);
    assert_eq!(metrics.statements, 1);
}

#[test]
fn file_metrics_count_impl_methods_and_private_import_names() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        "use std::{{fmt, io::Write}};\nstruct S;\nimpl S {{ fn a(&self) {{ let x = 1; }} fn b(&self) {{ let y = 2; }} }}",
    )
    .unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let metrics = compute_rust_file_metrics(&parsed);

    assert_eq!(metrics.imports, 2);
    assert_eq!(metrics.concrete_types, 1);
    assert_eq!(metrics.functions, 2);
    assert_eq!(metrics.statements, 2);
}

#[test]
fn accumulate_file_metrics_visits_each_top_level_item_kind_directly() {
    let file: syn::File = syn::parse_str(
        "trait T {}\nstruct S;\nenum E { A }\nunion U { a: u8 }\nuse std::{fmt, io::Write};\nfn f() { let x = 1; }\n",
    )
    .unwrap();
    let mut metrics = RustFileMetrics::default();

    accumulate_rust_file_metrics_from_items(&file.items, &mut metrics);

    assert_eq!(metrics.interface_types, 1);
    assert_eq!(metrics.concrete_types, 3);
    assert_eq!(metrics.imports, 2);
    assert_eq!(metrics.functions, 1);
    assert_eq!(metrics.statements, 1);
}
