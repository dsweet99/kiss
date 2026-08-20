use kiss::minhash::{compute_minhash, estimate_similarity, generate_shingles, normalize_code};
use kiss::parsing::{create_parser, parse_file};
use kiss::py_metrics::{compute_file_metrics, compute_function_metrics};
use std::fmt::Write as _;
use std::io::Write;

pub fn parse(code: &str) -> kiss::ParsedFile {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "{code}").unwrap();
    parse_file(&mut create_parser().unwrap(), tmp.path()).unwrap()
}

fn get_func_node(p: &kiss::ParsedFile) -> tree_sitter::Node<'_> {
    p.tree.root_node().child(0).unwrap()
}

#[test]
fn h1_syntax_error_in_function_body_still_produces_metrics() {
    let code = "def foo():\n    x = 1\n    if True\n        y = 2\n    return x, y\n";
    let p = parse(code);
    let root = p.tree.root_node();

    let has_error = has_error_node(root);
    assert!(
        has_error,
        "Expected tree-sitter to produce ERROR nodes for broken syntax"
    );

    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);

    assert!(m.statements >= 1, "Should have at least 1 statement");
}

#[test]
fn h1_error_node_in_return_corrupts_return_value_count() {
    let code = "def foo():\n    return (a b)\n";
    let p = parse(code);

    let has_error = has_error_node(p.tree.root_node());
    assert!(has_error, "Expected ERROR node for `return (a b)`");

    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);
    assert!(
        m.has_error,
        "function with syntax error in return should set has_error"
    );
}

#[test]
fn h1_unclosed_string_corrupts_entire_function() {
    let code = "def foo():\n    x = '''\n    y = 1\n    return y\n\ndef bar():\n    return 1\n";
    let p = parse(code);
    let fm = compute_file_metrics(&p);

    assert!(
        fm.functions >= 1,
        "should parse at least one function despite unclosed string, functions={}",
        fm.functions
    );
}

fn has_error_node(node: tree_sitter::Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_error_node(child) {
            return true;
        }
    }
    false
}

#[test]
fn h2_numeric_literals_stay_distinct_after_normalization() {
    let code_a = "result = compute(123, 456, 789)\nif result > 100:\n    return result * 2\n";
    let code_b = "result = compute(999, 111, 222)\nif result > 500:\n    return result * 3\n";

    let norm_a = normalize_code(code_a);
    let norm_b = normalize_code(code_b);

    assert_ne!(
        norm_a, norm_b,
        "Numeric literals must remain distinct after normalization"
    );

    let shingles_a = generate_shingles(&norm_a, 3);
    let shingles_b = generate_shingles(&norm_b, 3);
    let sig_a = compute_minhash(&shingles_a, 100);
    let sig_b = compute_minhash(&shingles_b, 100);
    let sim = estimate_similarity(&sig_a, &sig_b);

    let min_sim = kiss::defaults::duplication::MIN_SIMILARITY;
    assert!(
        sim < min_sim,
        "Functions differing only in numbers must be below duplication threshold ({min_sim}), got {sim}"
    );
}

#[test]
fn h2_single_variable_rename_drops_similarity() {
    let code_a =
        "x = get_data()\ny = transform(x)\nz = validate(y)\nresult = process(z)\nreturn result\n";
    let code_b =
        "x = get_data()\ny = transform(x)\nz = validate(y)\noutput = process(z)\nreturn output\n";

    let norm_a = normalize_code(code_a);
    let norm_b = normalize_code(code_b);

    let shingles_a = generate_shingles(&norm_a, 3);
    let shingles_b = generate_shingles(&norm_b, 3);
    let sig_a = compute_minhash(&shingles_a, 100);
    let sig_b = compute_minhash(&shingles_b, 100);
    let sim = estimate_similarity(&sig_a, &sig_b);

    assert!(
        sim > 0.5,
        "Near-duplicate with single rename should still have >50% similarity, got {sim}"
    );
}

#[test]
fn h3_function_with_1000_statements() {
    let mut code = String::from("def mega():\n");
    for i in 0..1000 {
        let _ = writeln!(code, "    x_{i} = {i}");
    }
    code.push_str("    return x_0\n");

    let p = parse(&code);
    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);

    assert_eq!(
        m.statements, 1001,
        "Should count 1000 assignments + 1 return"
    );
    assert!(
        m.local_variables >= 1000,
        "Should track at least 1000 local variables, got {}",
        m.local_variables
    );
}

#[test]
fn h3_deeply_nested_indentation() {
    let mut code = String::from("def deep():\n");
    let mut indent = String::from("    ");
    for i in 0..50 {
        let _ = writeln!(code, "{indent}if x_{i}:");
        indent.push_str("    ");
    }
    let _ = writeln!(code, "{indent}return 1");

    let p = parse(&code);
    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);

    assert_eq!(
        m.max_indentation, 51,
        "Should report 51 levels of indentation (1 base + 50 ifs)"
    );
}

#[test]
fn h3_function_with_100_return_statements() {
    let mut code = String::from("def many_returns(x):\n");
    for i in 0..100 {
        let _ = writeln!(code, "    if x == {i}:\n        return {i}");
    }

    let p = parse(&code);
    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);

    assert_eq!(m.returns, 100, "Should count 100 return statements");
}

#[test]
fn h3_file_with_500_functions() {
    let mut code = String::new();
    for i in 0..500 {
        let _ = write!(code, "def f_{i}():\n    return {i}\n\n");
    }

    let p = parse(&code);
    let fm = compute_file_metrics(&p);
    assert_eq!(fm.functions, 500, "Should count 500 functions");
}
