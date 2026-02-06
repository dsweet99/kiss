use kiss::rust_fn_metrics::compute_rust_function_metrics;

fn parse_first_fn(code: &str) -> (syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>, syn::Block, usize) {
    let file = syn::parse_file(code).expect("parse rust file");
    for item in file.items {
        if let syn::Item::Fn(f) = item {
            let inputs = f.sig.inputs;
            let block = *f.block;
            let attr_count = f.attrs.len();
            return (inputs, block, attr_count);
        }
    }
    panic!("expected fn");
}

#[test]
fn bug_rust_local_variables_should_count_typed_tuple_pattern_bindings() {
    // RULE: [Rust] [local_variables_per_function] counts local bindings introduced in a function.
    //
    // Hypothesis: typed patterns like `let (a, b): (i32, i32) = ...` are not counted.
    // Prediction: local_variables should be 2 (a and b), but current implementation reports 0.
    let (inputs, block, attr_count) = parse_first_fn(
        "fn f() {\n    let (a, b): (i32, i32) = (1, 2);\n    let _ = (a, b);\n}\n",
    );
    let m = compute_rust_function_metrics(&inputs, &block, attr_count);
    assert_eq!(m.local_variables, 2);
}

