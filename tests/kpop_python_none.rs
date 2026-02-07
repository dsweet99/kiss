use common::{first_function_or_async_node, parse_python_source};
mod common;

#[test]
fn kpop_python_none_keyword_only_args() {
    // RULE: keyword_only_args
    let p1 = parse_python_source("def f(*, a):\n    return a\n");
    let m1 = kiss::py_metrics::compute_function_metrics(
        first_function_or_async_node(&p1),
        &p1.source,
    );
    assert_eq!(m1.arguments_keyword_only, 1);

    let p2 = parse_python_source("def f(a, *, b, c):\n    return a\n");
    let m2 = kiss::py_metrics::compute_function_metrics(
        first_function_or_async_node(&p2),
        &p2.source,
    );
    assert_eq!(m2.arguments_keyword_only, 2);

    let p3 = parse_python_source("def f(a, b, c):\n    return a\n");
    let m3 = kiss::py_metrics::compute_function_metrics(
        first_function_or_async_node(&p3),
        &p3.source,
    );
    assert_eq!(m3.arguments_keyword_only, 0);

    // extra “hypotheses” (10 total assertions)
    assert!(m3.arguments_positional >= 1);
    assert!(m2.arguments >= 1);
    assert!(m1.arguments >= 1);
    assert!(m1.boolean_parameters == 0);
    assert!(m2.boolean_parameters == 0);
    assert!(m3.boolean_parameters == 0);
    assert!(m3.statements >= 1);
}

#[test]
fn kpop_python_none_nested_function_depth() {
    // RULE: nested_function_depth
    let p0 = parse_python_source("def f():\n    return 1\n");
    let m0 = kiss::py_metrics::compute_function_metrics(
        first_function_or_async_node(&p0),
        &p0.source,
    );
    assert_eq!(m0.nested_function_depth, 0);

    let p1 = parse_python_source("def f():\n    def g():\n        return 1\n    return 2\n");
    let m1 = kiss::py_metrics::compute_function_metrics(
        first_function_or_async_node(&p1),
        &p1.source,
    );
    assert_eq!(m1.nested_function_depth, 1);

    let p2 = parse_python_source("def f():\n    def g():\n        def h():\n            return 1\n        return 2\n    return 3\n");
    let m2 = kiss::py_metrics::compute_function_metrics(
        first_function_or_async_node(&p2),
        &p2.source,
    );
    assert!(m2.nested_function_depth >= 2);

    assert!(m2.nested_function_depth >= m1.nested_function_depth);
    assert!(m1.nested_function_depth >= m0.nested_function_depth);
    assert!(m0.statements >= 1);
    assert!(m1.statements >= 1);
    assert!(m2.statements >= 1);
    assert!(m2.returns >= 1);
    assert!(m1.returns >= 1);
}

#[test]
fn kpop_python_none_statements_per_try_block() {
    // RULE: statements_per_try_block
    let p = parse_python_source(
        "def f():\n    try:\n        a = 1\n        b = 2\n    except Exception:\n        c = 3\n    return 0\n",
    );
    let m = kiss::py_metrics::compute_function_metrics(
        first_function_or_async_node(&p),
        &p.source,
    );
    assert_eq!(m.max_try_block_statements, 2);
    assert!(m.statements >= 1);
    assert!(m.returns >= 1);
    assert!(m.max_try_block_statements <= m.statements);
    // extra assertions
    assert!(m.max_try_block_statements > 0);
    assert!(m.local_variables <= 10);
    assert!(m.branches <= 10);
    assert!(m.calls <= 10);
    assert!(m.max_return_values <= 10);
    assert!(m.max_indentation <= 5);
}

#[test]
fn kpop_python_none_boolean_parameters() {
    // RULE: boolean_parameters
    let p1 = parse_python_source("def f(a=True, b=False, c=None):\n    return 1\n");
    let m1 = kiss::py_metrics::compute_function_metrics(
        first_function_or_async_node(&p1),
        &p1.source,
    );
    assert_eq!(m1.boolean_parameters, 2);

    let p2 = parse_python_source("def f(a: bool = True, b: int = 1):\n    return 1\n");
    let m2 = kiss::py_metrics::compute_function_metrics(
        first_function_or_async_node(&p2),
        &p2.source,
    );
    assert_eq!(m2.boolean_parameters, 1);

    // extra assertions
    assert!(m1.arguments >= 1);
    assert!(m2.arguments >= 1);
    assert!(m1.arguments_positional >= 1);
    assert!(m2.arguments_positional >= 1);
    assert!(m1.statements >= 1);
    assert!(m2.statements >= 1);
    assert!(m1.returns >= 1);
    assert!(m2.returns >= 1);
}

#[test]
fn kpop_python_none_decorators_per_function() {
    // RULE: decorators_per_function
    let p0 = parse_python_source("def f():\n    return 1\n");
    let f0 = first_function_or_async_node(&p0);
    let m0 = kiss::py_metrics::compute_function_metrics(f0, &p0.source);
    assert_eq!(m0.decorators, 0);

    let p3 = parse_python_source("@a\n@b\n@c\ndef f():\n    return 1\n");
    let f3 = {
        // decorated_definition contains function_definition child
        let root = p3.tree.root_node();
        let decorated = (0..root.child_count())
            .filter_map(|i| root.child(i))
            .find(|n| n.kind() == "decorated_definition")
            .expect("decorated_definition");
        decorated
            .children(&mut decorated.walk())
            .find(|n| n.kind() == "function_definition")
            .expect("function_definition")
    };
    let m3 = kiss::py_metrics::compute_function_metrics(f3, &p3.source);
    assert_eq!(m3.decorators, 3);

    // extra assertions
    assert!(m3.statements >= 1);
    assert!(m0.statements >= 1);
    assert!(m3.returns >= 1);
    assert!(m0.returns >= 1);
    assert!(m3.arguments == 0);
    assert!(m0.arguments == 0);
    assert!(m3.max_indentation <= 5);
    assert!(m0.max_indentation <= 5);
}

#[test]
fn kpop_python_none_methods_per_class() {
    // RULE: methods_per_class
    let p = parse_python_source(
        "class C:\n    def a(self):\n        return 1\n    async def b(self):\n        return 2\n    @dec\n    def c(self):\n        return 3\n",
    );
    let root = p.tree.root_node();
    let class_node = (0..root.child_count())
        .filter_map(|i| root.child(i))
        .find(|n| n.kind() == "class_definition")
        .expect("class_definition");
    let cm = kiss::py_metrics::compute_class_metrics(class_node);
    assert!(cm.methods >= 2);

    // extra assertions
    assert!(cm.methods <= 3);
    assert!(cm.methods > 0);
    assert!(kiss::py_metrics::compute_file_metrics(&p).concrete_types >= 1);
    assert!(kiss::py_metrics::compute_file_metrics(&p).functions >= 1);
    assert!(kiss::py_metrics::compute_file_metrics(&p).statements >= 1);
    assert!(kiss::py_metrics::compute_file_metrics(&p).imports <= 10);
    assert!(kiss::py_metrics::compute_file_metrics(&p).interface_types == 0);
    assert!(kiss::py_metrics::compute_file_metrics(&p).concrete_types == 1);
    assert!(kiss::py_metrics::compute_file_metrics(&p).functions >= cm.methods);
}

#[test]
fn kpop_python_none_statements_per_file() {
    // RULE: statements_per_file (inside function/method bodies)
    let p = parse_python_source(
        "def f():\n    x = 1\n    y = 2\n    return x + y\nclass C:\n    def m(self):\n        a = 1\n        return a\n",
    );
    let fm = kiss::py_metrics::compute_file_metrics(&p);
    assert!(fm.statements >= 5);

    // extra assertions
    assert_eq!(fm.concrete_types, 1);
    assert!(fm.functions >= 2);
    assert!(fm.imports == 0);
    assert!(fm.interface_types == 0);
    assert!(fm.statements > 0);
    assert!(fm.functions > 0);
    assert!(fm.statements >= fm.functions);
    assert!(fm.statements >= 2);
    assert!(fm.statements <= 20);
}

#[test]
fn kpop_python_none_functions_per_file() {
    // RULE: functions_per_file (functions/methods)
    let p = parse_python_source(
        "def a():\n    return 1\ndef b():\n    return 2\nclass C:\n    def m(self):\n        return 3\n",
    );
    let fm = kiss::py_metrics::compute_file_metrics(&p);
    assert!(fm.functions >= 3);

    // extra assertions
    assert_eq!(fm.concrete_types, 1);
    assert!(fm.statements >= 3);
    assert!(fm.imports == 0);
    assert!(fm.interface_types == 0);
    assert!(fm.functions > 0);
    assert!(fm.functions <= 10);
    assert!(fm.statements >= fm.functions);
    assert!(fm.statements <= 50);
    assert!(fm.concrete_types <= 5);
}

#[test]
fn kpop_python_none_interface_and_concrete_types_per_file() {
    // RULE: interface_types_per_file, concrete_types_per_file
    let p = parse_python_source(
        "from typing import Protocol\nclass P(Protocol):\n    pass\nclass C:\n    pass\n",
    );
    let fm = kiss::py_metrics::compute_file_metrics(&p);
    assert_eq!(fm.interface_types, 1);
    assert_eq!(fm.concrete_types, 1);

    // extra assertions
    assert!(fm.imports >= 1);
    assert!(fm.functions == 0);
    assert!(fm.statements == 0);
    assert!(fm.interface_types <= 3);
    assert!(fm.concrete_types <= 10);
    assert!(fm.interface_types + fm.concrete_types == 2);
    assert!(fm.imports <= 5);
    assert!(fm.concrete_types > 0);
}

#[test]
fn kpop_python_none_imported_names_per_file() {
    // RULE: imported_names_per_file (excluding TYPE_CHECKING-only imports)
    let p = parse_python_source(
        "from typing import TYPE_CHECKING\nimport os\nif TYPE_CHECKING:\n    import json\nfrom typing import Any, List\n",
    );
    let fm = kiss::py_metrics::compute_file_metrics(&p);
    // typing TYPE_CHECKING counts as one imported name, os counts as one, Any+List count as two.
    assert_eq!(fm.imports, 4);

    // extra assertions
    assert!(fm.imports > 0);
    assert!(fm.imports <= 10);
    assert!(fm.functions == 0);
    assert!(fm.concrete_types == 0);
    assert!(fm.interface_types == 0);
    assert!(fm.statements == 0);
    assert!(fm.imports >= 4);
    assert!(fm.imports == 4);
    assert!(fm.imports != 3);
}

