use crate::common::{first_function_node, parse_python_source};
use kiss::py_metrics::compute_function_metrics;

#[test]
fn bug_python_function_metrics_should_not_count_nested_function_bodies() {
    let code = r"
def outer():
    def inner():
        x = 1
        if x:
            print(x)
            return x
        return 0
    return 1
";
    let p = parse_python_source(code);
    let outer = first_function_node(&p);
    let m = compute_function_metrics(outer, &p.source);

    assert_eq!(m.returns, 1, "outer returns should ignore inner returns");
    assert_eq!(m.branches, 0, "outer branches should ignore inner if");
    assert_eq!(m.calls, 0, "outer calls should ignore inner print()");
    assert_eq!(
        m.local_variables, 0,
        "outer locals should ignore inner assignments"
    );
    assert_eq!(
        m.statements, 1,
        "outer statements should ignore nested function body statements"
    );
    assert_eq!(
        m.max_indentation, 1,
        "outer indentation depth should ignore nested function body indentation"
    );
}

#[test]
fn bug_return_values_per_function_parenthesized_tuple_counts_elements() {
    let p = parse_python_source("def f():\n    return (1, 2, 3)\n");
    let func = first_function_node(&p);
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.max_return_values, 3);
}

#[test]
fn bug_methods_per_class_should_count_async_methods() {
    let p = parse_python_source("class C:\n    async def m(self):\n        return 1\n");
    let root = p.tree.root_node();
    let class_node = (0..root.child_count())
        .filter_map(|i| root.child(i))
        .find(|n| n.kind() == "class_definition")
        .expect("class_definition");
    let m = kiss::py_metrics::compute_class_metrics(class_node);
    assert_eq!(m.methods, 1);
}

#[test]
fn bug_methods_per_class_counts_both_sync_and_async() {
    let p = parse_python_source(
        "class C:\n    def a(self):\n        return 1\n    async def b(self):\n        return 2\n",
    );
    let root = p.tree.root_node();
    let class_node = (0..root.child_count())
        .filter_map(|i| root.child(i))
        .find(|n| n.kind() == "class_definition")
        .expect("class_definition");
    let m = kiss::py_metrics::compute_class_metrics(class_node);
    assert_eq!(m.methods, 2);
}

#[test]
fn bug_positional_args_should_count_varargs_parameter() {
    let p = parse_python_source("def f(*args):\n    return 1\n");
    let func = first_function_node(&p);
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.arguments_positional, 1);
}
