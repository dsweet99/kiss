use common::{first_function_node, parse_python_source};
use kiss::py_metrics::compute_function_metrics;
mod common;

#[test]
fn bug_python_function_metrics_should_not_count_nested_function_bodies() {
    // Targets multiple Python rules that depend on "inside the function body":
    // - statements_per_function
    // - branches_per_function
    // - local_variables_per_function
    // - returns_per_function
    // - calls_per_function
    //
    // Hypothesis: metrics for a function include nodes inside nested function bodies.
    // Prediction: only the outer `return` should contribute to the outer function counts.
    // Test: ensure nested function body doesn't affect outer metrics.
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

    // Expected: outer() has exactly one return statement and no branches/calls/locals beyond the return literal.
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
    // RULE: [Python] [return_values_per_function] counts values returned by a single return statement.
    //
    // Hypothesis: `return (a, b, c)` is treated as a single return value.
    // Prediction: max_return_values should be 3, but is currently 1.
    let p = parse_python_source("def f():\n    return (1, 2, 3)\n");
    let func = first_function_node(&p);
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.max_return_values, 3);
}

#[test]
fn bug_methods_per_class_should_count_async_methods() {
    // RULE: [Python] [methods_per_class] is the maximum number of methods defined on a Python class.
    //
    // Hypothesis: async methods are not counted as methods.
    // Prediction: a class with only an `async def` method should have methods==1.
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
    // Hypothesis: async methods are not counted.
    // Prediction: a class with one sync and one async method has methods==2.
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
    // RULE: [Python] [positional_args] is the maximum number of positional parameters.
    //
    // Hypothesis: `*args` is not counted as a positional parameter.
    // Prediction: `def f(*args): ...` should have 1 positional parameter.
    let p = parse_python_source("def f(*args):\n    return 1\n");
    let func = first_function_node(&p);
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.arguments_positional, 1);
}

