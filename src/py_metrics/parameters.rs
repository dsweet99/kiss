use tree_sitter::Node;

pub(crate) struct ParameterCounts {
    pub(crate) positional: usize,
    pub(crate) keyword_only: usize,
    pub(crate) total: usize,
    pub(crate) boolean_params: usize,
}

fn is_self_or_cls_param(n: Node, source: &str) -> bool {
    let text = n.utf8_text(source.as_bytes()).unwrap_or("");
    matches!(text, "self" | "cls")
}

fn count_typed_parameter(
    child: Node,
    source: &str,
    positional: &mut usize,
    after_star: &mut bool,
) -> bool {
    if child
        .child_by_field_name("name")
        .is_some_and(|n| is_self_or_cls_param(n, source))
    {
        return true;
    }
    if child
        .child(0)
        .is_some_and(|c| c.kind() == "list_splat_pattern")
    {
        *positional += 1;
        *after_star = true;
        return true;
    }
    if child
        .child(0)
        .is_some_and(|c| c.kind() == "dictionary_splat_pattern")
    {
        *after_star = true;
        return true;
    }
    false
}

fn count_default_parameter(
    child: Node,
    source: &str,
    positional: &mut usize,
    keyword_only: &mut usize,
    after_star: &mut bool,
    boolean_params: &mut usize,
) {
    if *after_star {
        *keyword_only += 1;
    } else {
        *positional += 1;
    }
    if is_boolean_param(&child, source) {
        *boolean_params += 1;
    }
}

fn count_one_parameter(
    child: Node,
    source: &str,
    positional: &mut usize,
    keyword_only: &mut usize,
    after_star: &mut bool,
    boolean_params: &mut usize,
) {
    match child.kind() {
        "identifier" if is_self_or_cls_param(child, source) => {}
        "typed_parameter" if count_typed_parameter(child, source, positional, after_star) => {}
        "identifier" | "typed_parameter" => {
            if *after_star {
                *keyword_only += 1;
            } else {
                *positional += 1;
            }
            if is_boolean_param(&child, source) {
                *boolean_params += 1;
            }
        }
        "default_parameter" | "typed_default_parameter" => {
            count_default_parameter(
                child,
                source,
                positional,
                keyword_only,
                after_star,
                boolean_params,
            );
        }
        "list_splat_pattern" => {
            *positional += 1;
            *after_star = true;
        }
        "dictionary_splat_pattern" | "*" | "keyword_separator" => *after_star = true,
        _ => {}
    }
}

pub(crate) fn count_parameters(params: Node, source: &str) -> ParameterCounts {
    let (mut positional, mut keyword_only, mut after_star, mut boolean_params) = (0, 0, false, 0);
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        count_one_parameter(
            child,
            source,
            &mut positional,
            &mut keyword_only,
            &mut after_star,
            &mut boolean_params,
        );
    }
    ParameterCounts {
        positional,
        keyword_only,
        total: positional + keyword_only,
        boolean_params,
    }
}

pub(crate) fn is_boolean_default(param: &Node, source: &str) -> bool {
    param.child_by_field_name("value").is_some_and(|v| {
        let text = v.utf8_text(source.as_bytes()).unwrap_or("");
        matches!(text, "True" | "False")
    })
}

fn is_bool_annotation(param: &Node, source: &str) -> bool {
    param
        .child_by_field_name("type")
        .is_some_and(|ty| ty.utf8_text(source.as_bytes()).unwrap_or("") == "bool")
}

fn is_boolean_param(param: &Node, source: &str) -> bool {
    is_bool_annotation(param, source) || is_boolean_default(param, source)
}

pub(crate) fn count_decorators(node: Node) -> usize {
    node.parent()
        .filter(|p| p.kind() == "decorated_definition")
        .map_or(0, |p| {
            p.children(&mut p.walk())
                .filter(|c| c.kind() == "decorator")
                .count()
        })
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use crate::test_utils::parse_python_source;

    impl ParameterCounts {
        fn witness() -> Self {
            Self {
                positional: 0,
                keyword_only: 0,
                total: 0,
                boolean_params: 0,
            }
        }
    }

    #[test]
    fn witness_parameter_counts() {
        let _ = ParameterCounts::witness();
        let parsed = parse_python_source("def f(a, b=1, *, c): pass");
        let func = parsed.tree.root_node().child(0).expect("function");
        let params = func.child_by_field_name("parameters").expect("params");
        let counts = count_parameters(params, &parsed.source);
        assert!(counts.total >= 1);
    }

    #[test]
    fn typed_self_cls_and_typed_splats_follow_parameter_rules() {
        let parsed = parse_python_source(
            "def f(self: object, cls: type, a: int, *args: str, b: bool = False, **kwargs: object): pass",
        );
        let func = parsed.tree.root_node().child(0).expect("function");
        let params = func.child_by_field_name("parameters").expect("params");
        let counts = count_parameters(params, &parsed.source);

        assert_eq!(counts.positional, 4);
        assert_eq!(counts.keyword_only, 1);
        assert_eq!(counts.total, 5);
        assert_eq!(counts.boolean_params, 1);
    }

    #[test]
    fn decorator_count_counts_multiple_parent_decorators() {
        let parsed = parse_python_source("@one\n@two\ndef f(): pass");
        let decorated = parsed.tree.root_node().child(0).expect("decorated");
        let function = decorated
            .children(&mut decorated.walk())
            .find(|node| node.kind() == "function_definition")
            .expect("function");

        assert_eq!(count_decorators(function), 2);
    }

    #[test]
    fn parameter_counts_handle_plain_and_typed_keyword_only_forms() {
        let parsed = parse_python_source(
            "def f(self, a, *args, b=False, c: int = 1, **kwargs):\n    pass\n",
        );
        let func = parsed.tree.root_node().child(0).expect("function");
        let params = func.child_by_field_name("parameters").expect("params");

        let counts = count_parameters(params, &parsed.source);

        assert_eq!(counts.positional, 2);
        assert_eq!(counts.keyword_only, 2);
        assert_eq!(counts.total, 4);
        assert_eq!(counts.boolean_params, 1);
    }

    #[test]
    fn boolean_default_returns_false_for_non_default_parameter() {
        let parsed = parse_python_source("def f(a):\n    pass\n");
        let func = parsed.tree.root_node().child(0).expect("function");
        let params = func.child_by_field_name("parameters").expect("params");
        let ident = params
            .children(&mut params.walk())
            .find(|child| child.kind() == "identifier")
            .expect("identifier");

        assert!(!is_boolean_default(&ident, &parsed.source));
    }

    #[test]
    fn boolean_param_counts_bool_typed_args_without_defaults() {
        let parsed = parse_python_source(
            "def f(verbose: bool, flag: bool = False, n: int = 1):\n    pass\n",
        );
        let func = parsed.tree.root_node().child(0).expect("function");
        let params = func.child_by_field_name("parameters").expect("params");
        let counts = count_parameters(params, &parsed.source);
        assert_eq!(counts.boolean_params, 2);
    }
}
