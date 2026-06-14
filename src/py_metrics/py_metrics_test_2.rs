use crate::test_utils::parse_python_source as parse;

use super::compute::compute_function_metrics;
use super::file_walk::is_interface_token;
use super::parameters::count_parameters;
use super::returns::count_return_values;

fn get_func_node(p: &crate::parsing::ParsedFile) -> tree_sitter::Node<'_> {
    p.tree.root_node().child(0).unwrap()
}

#[cfg_attr(test, allow(dead_code))]
fn compute_max_return_values(node: tree_sitter::Node) -> usize {
    let mut max = if node.kind() == "return_statement" {
        count_return_values(node)
    } else {
        0
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        max = max.max(compute_max_return_values(child));
    }
    max
}

#[test]
fn test_compute_max_return_values() {
    let p = parse("def f():\n    return 1, 2\n    if x:\n        return 3\n");
    let func = get_func_node(&p);
    let body = func.child_by_field_name("body").unwrap();
    assert!(compute_max_return_values(body) >= 2);
}

#[test]
fn test_compute_function_metrics_self_not_counted() {
    let p = parse("class C:\n    def method(self, a, b): pass");
    let cls = p.tree.root_node().child(0).unwrap();
    let body = cls.child_by_field_name("body").unwrap();
    let method = body.child(0).unwrap();
    let m = compute_function_metrics(method, &p.source);
    assert_eq!(
        m.arguments_positional, 2,
        "self should not be counted as positional arg (got {})",
        m.arguments_positional
    );
}

#[test]
fn test_is_interface_token() {
    assert!(is_interface_token("Protocol"));
    assert!(is_interface_token("ABC"));
    assert!(is_interface_token("ABCMeta"));
    assert!(!is_interface_token("BaseClass"));
    assert!(!is_interface_token("object"));
}

#[test]
fn test_typed_star_args_counts_correctly() {
    let p = parse("def f(*args: object, a: bool = True, b: int = 0): pass");
    let params = get_func_node(&p).child_by_field_name("parameters").unwrap();
    let counts = count_parameters(params, &p.source);
    assert_eq!(
        counts.positional, 1,
        "typed *args should count as 1 positional"
    );
    assert_eq!(
        counts.keyword_only, 2,
        "params after typed *args should be keyword-only"
    );
}

#[test]
fn test_untyped_star_args_counts_correctly() {
    let p = parse("def f(*args, a=True, b=0): pass");
    let params = get_func_node(&p).child_by_field_name("parameters").unwrap();
    let counts = count_parameters(params, &p.source);
    assert_eq!(
        counts.positional, 1,
        "untyped *args should count as 1 positional"
    );
    assert_eq!(
        counts.keyword_only, 2,
        "params after untyped *args should be keyword-only"
    );
}

#[test]
fn test_typed_star_args_with_leading_positional() {
    let p = parse("def f(x, y, *args: tuple, kw: int = 0): pass");
    let params = get_func_node(&p).child_by_field_name("parameters").unwrap();
    let counts = count_parameters(params, &p.source);
    assert_eq!(counts.positional, 3, "x, y, *args = 3 positional");
    assert_eq!(
        counts.keyword_only, 1,
        "kw after *args should be keyword-only"
    );
}

#[test]
fn test_typed_kwargs_not_counted() {
    let p = parse("def f(a, **kwargs: dict): pass");
    let params = get_func_node(&p).child_by_field_name("parameters").unwrap();
    let counts = count_parameters(params, &p.source);
    assert_eq!(counts.positional, 1, "only a is positional");
    assert_eq!(counts.keyword_only, 0, "**kwargs should not be counted");
}
