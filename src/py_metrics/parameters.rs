use tree_sitter::Node;

pub(crate) struct ParameterCounts {
    pub(crate) positional: usize,
    pub(crate) keyword_only: usize,
    pub(crate) total: usize,
    pub(crate) boolean_params: usize,
}

pub(crate) fn count_parameters(params: Node, source: &str) -> ParameterCounts {
    let is_self_or_cls = |n: Node| {
        let text = n.utf8_text(source.as_bytes()).unwrap_or("");
        matches!(text, "self" | "cls")
    };
    let (mut positional, mut keyword_only, mut after_star, mut boolean_params) = (0, 0, false, 0);
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        match child.kind() {
            "identifier" if is_self_or_cls(child) => {
                // Skip self/cls - they are not real parameters
            }
            "typed_parameter"
                if child
                    .child_by_field_name("name")
                    .is_some_and(&is_self_or_cls) =>
            {
                // Skip typed self/cls parameters (e.g., self: SomeType)
            }
            "typed_parameter"
                if child
                    .child(0)
                    .is_some_and(|c| c.kind() == "list_splat_pattern") =>
            {
                positional += 1;
                after_star = true;
            }
            "typed_parameter"
                if child
                    .child(0)
                    .is_some_and(|c| c.kind() == "dictionary_splat_pattern") =>
            {
                after_star = true;
            }
            "identifier" | "typed_parameter" => {
                if after_star {
                    keyword_only += 1;
                } else {
                    positional += 1;
                }
            }
            "default_parameter" | "typed_default_parameter" => {
                if after_star {
                    keyword_only += 1;
                } else {
                    positional += 1;
                }
                if is_boolean_default(&child, source) {
                    boolean_params += 1;
                }
            }
            "list_splat_pattern" => {
                positional += 1;
                after_star = true;
            }
            "dictionary_splat_pattern" | "*" | "keyword_separator" => after_star = true,
            _ => {}
        }
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

pub(crate) fn count_decorators(node: Node) -> usize {
    node.parent()
        .filter(|p| p.kind() == "decorated_definition")
        .map_or(0, |p| {
            p.children(&mut p.walk())
                .filter(|c| c.kind() == "decorator")
                .count()
        })
}
