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
    if is_boolean_default(&child, source) {
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

pub(crate) fn count_decorators(node: Node) -> usize {
    node.parent()
        .filter(|p| p.kind() == "decorated_definition")
        .map_or(0, |p| {
            p.children(&mut p.walk())
                .filter(|c| c.kind() == "decorator")
                .count()
        })
}
