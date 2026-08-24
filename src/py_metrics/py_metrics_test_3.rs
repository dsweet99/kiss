#![allow(unused_imports, dead_code)]
use tree_sitter::Node;

use crate::test_utils::parse_python_source as parse;

use super::body_walk::analyze_body;
use super::compute::compute_function_metrics;
use super::compute_file_metrics;
use super::file_stats::{count_class_statements, count_file_statements};
use super::file_walk::{collect_file_counts, is_interface_type};
use super::locals::collect_assigned_names;
use super::nesting::compute_nested_function_depth;
use super::parameters::{ParameterCounts, count_decorators, count_parameters, is_boolean_default};
use super::returns::count_return_values;
use super::statements::{count_statements, is_statement};

fn get_func_node(p: &crate::parsing::ParsedFile) -> Node<'_> {
    p.tree.root_node().child(0).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_touch_return_helpers_for_static_coverage() {
        let p_ret = parse("def g():\n    return a, b, c");
        let ret = p_ret
            .tree
            .root_node()
            .child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap()
            .child(0)
            .unwrap();
        assert_eq!(count_return_values(ret), 3);
    }

    #[test]
    fn test_touch_statement_counters_for_static_coverage() {
        let p2 = parse("class C:\n    def m(self):\n        x = 1\n        return x\n");
        let root2 = p2.tree.root_node();
        assert!(count_file_statements(root2) > 0);
        let class_body = root2.child(0).unwrap().child_by_field_name("body").unwrap();
        assert!(count_class_statements(class_body) > 0);
    }

    #[test]
    fn file_statement_counts_include_async_and_decorated_definitions() {
        let parsed = parse(
            "@decorator\nasync def f():\n    x = 1\n\nclass C:\n    @decorator\n    async def m(self):\n        y = 2\n",
        );
        let root = parsed.tree.root_node();
        assert_eq!(count_file_statements(root), 2);

        let class_body = root
            .children(&mut root.walk())
            .find(|node| node.kind() == "class_definition")
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(count_class_statements(class_body), 1);
    }

    #[test]
    fn parameter_counts_handle_self_cls_splats_and_keyword_only_defaults() {
        let parsed = parse(
            "class C:\n    def m(self, cls, a: int, *args, b=True, **kwargs):\n        pass\n",
        );
        let class_body = parsed
            .tree
            .root_node()
            .child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let method = class_body
            .children(&mut class_body.walk())
            .find(|node| node.kind() == "function_definition")
            .unwrap();
        let params = method.child_by_field_name("parameters").unwrap();
        let counts = count_parameters(params, &parsed.source);

        assert_eq!(counts.positional, 2);
        assert_eq!(counts.keyword_only, 1);
        assert_eq!(counts.boolean_params, 1);
        assert_eq!(counts.total, 3);
    }

    #[test]
    fn decorator_count_returns_zero_for_plain_function() {
        let parsed = parse("def f():\n    pass\n");
        assert_eq!(count_decorators(get_func_node(&parsed)), 0);
    }

    #[test]
    fn class_metrics_default_when_body_is_missing_or_not_class_body() {
        let parsed = parse("x = 1\n");
        let root = parsed.tree.root_node();

        assert_eq!(crate::py_metrics::compute_class_metrics(root).methods, 0);
    }

    #[test]
    fn file_counts_classify_non_interface_classes_and_skip_type_checking_imports() {
        let parsed = parse(
            "import typing\nif typing.TYPE_CHECKING:\n    import hidden\nclass Concrete(Base):\n    pass\n",
        );
        let counts = collect_file_counts(parsed.tree.root_node(), &parsed.source);

        assert_eq!(counts.concrete_types, 1);
        assert_eq!(counts.interface_types, 0);
        assert!(counts.import_names.contains("typing"));
        assert!(!counts.import_names.contains("hidden"));
    }

    #[test]
    fn parameter_counts_handle_bare_separator_and_dictionary_splat() {
        let parsed = parse("def f(a, *, b, **kwargs):\n    pass\n");
        let params = get_func_node(&parsed)
            .child_by_field_name("parameters")
            .unwrap();
        let counts = count_parameters(params, &parsed.source);

        assert_eq!(counts.positional, 1);
        assert_eq!(counts.keyword_only, 1);
        assert_eq!(counts.total, 2);
    }

    #[test]
    fn file_statement_counts_handle_plain_async_and_nested_decorated_methods() {
        let parsed = parse(
            "async def top():\n    x = 1\n\nclass C:\n    @decorator\n    def m(self):\n        y = 2\n",
        );
        let root = parsed.tree.root_node();
        assert_eq!(count_file_statements(root), 2);

        let class_body = root
            .children(&mut root.walk())
            .find(|node| node.kind() == "class_definition")
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(count_class_statements(class_body), 1);
    }

    #[test]
    fn parameter_counts_handle_typed_keyword_only_and_splat_boundaries() {
        let parsed = parse(
            "def f(cls: type, a: int, *args: str, b: bool = True, **kwargs: object):\n    pass\n",
        );
        let params = get_func_node(&parsed)
            .child_by_field_name("parameters")
            .unwrap();
        let counts = count_parameters(params, &parsed.source);

        assert_eq!(counts.positional, 3);
        assert_eq!(counts.keyword_only, 1);
        assert_eq!(counts.total, 4);
        assert_eq!(counts.boolean_params, 1);
    }
}
