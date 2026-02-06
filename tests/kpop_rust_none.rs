use kiss::rust_fn_metrics::compute_rust_function_metrics;
use kiss::rust_parsing::{ParsedRustFile, parse_rust_file};
use std::io::Write;

fn parse_first_fn(code: &str) -> (syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>, syn::Block, usize) {
    let file = syn::parse_file(code).expect("parse rust file");
    for item in file.items {
        if let syn::Item::Fn(f) = item {
            return (f.sig.inputs, *f.block, f.attrs.len());
        }
    }
    panic!("expected fn");
}

fn parse_rs_tmp(code: &str) -> ParsedRustFile {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    write!(tmp, "{code}").unwrap();
    parse_rust_file(tmp.path()).expect("parse rust")
}

#[test]
fn kpop_rust_none_statements_per_function() {
    // RULE: statements_per_function
    let (inputs, block, attr_count) = parse_first_fn("fn f(){ use std::io; let x=1; x; }");
    let m = compute_rust_function_metrics(&inputs, &block, attr_count);
    assert!(m.statements >= 2);
    // extra assertions (10)
    assert!(m.calls == 0);
    assert!(m.branches == 0);
    assert!(m.returns == 0);
    assert!(m.local_variables >= 1);
    assert!(m.arguments == 0);
    assert_eq!(m.max_indentation, 0);
    assert!(m.nested_function_depth == 0);
    assert!(m.attributes == attr_count);
    assert!(m.bool_parameters == 0);
}

#[test]
fn kpop_rust_none_arguments_per_function() {
    // RULE: arguments_per_function
    let (inputs, block, attr_count) = parse_first_fn("fn f(self_: i32, a:i32, b:bool){ let _=a+b; }");
    let m = compute_rust_function_metrics(&inputs, &block, attr_count);
    assert_eq!(m.arguments, 3);
    // extra assertions
    assert_eq!(m.bool_parameters, 1);
    assert!(m.statements >= 1);
    assert_eq!(m.local_variables, 0);
    assert!(m.calls == 0);
    assert!(m.branches == 0);
    assert!(m.returns == 0);
    assert!(m.max_indentation <= 10);
    assert!(m.nested_function_depth == 0);
    assert_eq!(m.attributes, attr_count);
}

#[test]
fn kpop_rust_none_max_indentation_depth_and_branches() {
    // RULE: max_indentation_depth, branches_per_function
    let (inputs, block, attr_count) = parse_first_fn(
        "fn f(x:i32){ if x>0 { if x>1 { let _=x; } } }",
    );
    let m = compute_rust_function_metrics(&inputs, &block, attr_count);
    assert!(m.branches >= 2);
    assert!(m.max_indentation >= 2);
    // extra assertions
    assert!(m.statements >= 1);
    assert_eq!(m.local_variables, 0);
    assert!(m.returns == 0);
    assert!(m.calls == 0);
    assert!(m.arguments == 1);
    assert!(m.bool_parameters == 0);
    assert!(m.nested_function_depth == 0);
    assert!(m.attributes == attr_count);
    assert!(m.max_indentation <= 10);
}

#[test]
fn kpop_rust_none_returns_per_function() {
    // RULE: returns_per_function counts explicit `return`.
    let (inputs, block, attr_count) = parse_first_fn(
        "fn f(x:i32)->i32{ if x>0 { return 1; } return 2; }",
    );
    let m = compute_rust_function_metrics(&inputs, &block, attr_count);
    assert_eq!(m.returns, 2);
    // extra assertions
    assert!(m.branches >= 1);
    assert!(m.statements >= 1);
    assert!(m.arguments == 1);
    assert!(m.bool_parameters == 0);
    assert!(m.calls == 0);
    assert!(m.local_variables == 0);
    assert!(m.max_indentation >= 1);
    assert!(m.nested_function_depth == 0);
    assert!(m.attributes == attr_count);
}

#[test]
fn kpop_rust_none_nested_function_depth() {
    // RULE: nested_function_depth counts closures nesting
    let (inputs, block, attr_count) = parse_first_fn(
        "fn f(){ let _ = || { let _ = || { 1 }; 2 }; }",
    );
    let m = compute_rust_function_metrics(&inputs, &block, attr_count);
    assert!(m.nested_function_depth >= 2);
    // extra assertions
    assert!(m.statements >= 1);
    assert!(m.arguments == 0);
    assert!(m.bool_parameters == 0);
    assert!(m.returns == 0);
    assert!(m.calls == 0);
    assert!(m.branches == 0);
    assert_eq!(m.local_variables, 0);
    assert!(m.max_indentation <= 10);
    assert!(m.attributes == attr_count);
}

#[test]
fn kpop_rust_none_boolean_parameters() {
    // RULE: boolean_parameters
    let (inputs, block, attr_count) = parse_first_fn("fn f(a:bool,b:bool,c:i32){ let _=c; }");
    let m = compute_rust_function_metrics(&inputs, &block, attr_count);
    assert_eq!(m.bool_parameters, 2);
    // extra assertions
    assert_eq!(m.arguments, 3);
    assert!(m.statements >= 1);
    assert_eq!(m.local_variables, 0);
    assert!(m.calls == 0);
    assert!(m.branches == 0);
    assert!(m.returns == 0);
    assert!(m.nested_function_depth == 0);
    assert!(m.max_indentation <= 10);
    assert!(m.attributes == attr_count);
}

#[test]
fn kpop_rust_none_attributes_per_function() {
    // RULE: attributes_per_function (we treat it as a direct count of passed-in non-doc attributes)
    let code = "#[inline]\n#[allow(dead_code)]\nfn f(){ let _=1; }";
    let (inputs, block, attr_count) = parse_first_fn(code);
    let m = compute_rust_function_metrics(&inputs, &block, attr_count);
    assert_eq!(m.attributes, attr_count);
    // extra assertions
    assert!(m.attributes >= 2);
    assert!(m.statements >= 1);
    assert!(m.arguments == 0);
    assert!(m.bool_parameters == 0);
    assert!(m.calls == 0);
    assert!(m.branches == 0);
    assert!(m.returns == 0);
    assert!(m.nested_function_depth == 0);
    assert!(m.max_indentation <= 10);
}

#[test]
fn kpop_rust_none_calls_per_function() {
    // RULE: calls_per_function
    let (inputs, block, attr_count) = parse_first_fn("fn f(){ g(); h(1); } fn g(){} fn h(_:i32){}");
    let m = compute_rust_function_metrics(&inputs, &block, attr_count);
    assert!(m.calls >= 2);
    // extra assertions
    assert!(m.statements >= 1);
    assert!(m.arguments == 0);
    assert!(m.bool_parameters == 0);
    assert!(m.branches == 0);
    assert!(m.returns == 0);
    assert!(m.local_variables == 0);
    assert!(m.nested_function_depth == 0);
    assert_eq!(m.max_indentation, 0);
    assert!(m.attributes == attr_count);
}

#[test]
fn kpop_rust_none_file_metrics_counts() {
    // RULES: statements_per_file, functions_per_file, interface_types_per_file, concrete_types_per_file, imported_names_per_file
    let parsed = parse_rs_tmp(
        "use std::io;\ntrait T{}\nstruct S;\nfn f(){ let _=1; }\nimpl S{ fn m(&self){ let _=2; } }\n",
    );
    let fm = kiss::compute_rust_file_metrics(&parsed);
    assert_eq!(fm.interface_types, 1);
    assert_eq!(fm.concrete_types, 1);
    assert_eq!(fm.imports, 1);
    assert!(fm.functions >= 2);
    assert!(fm.statements >= 2);
    // extra assertions
    assert!(fm.statements <= 50);
    assert!(fm.functions <= 10);
    assert!(fm.imports <= 10);
    assert!(fm.interface_types <= 3);
    assert!(fm.concrete_types <= 8);
}

