use std::ops::Not;
use tree_sitter::Node;

pub(super) fn is_py_const_true(node: Node, source: &str) -> bool {
    match node.kind() {
        "true" => true,
        "integer" => {
            let text = &source[node.start_byte()..node.end_byte()];
            text != "0"
        }
        "false" | "none" => false,
        "comparison_operator" => eval_py_comparison(node, source) == Some(true),
        "boolean_operator" => eval_py_boolean(node, source).not(),
        "unary_operator" => super::is_py_const_false(node, source),
        _ => false,
    }
}

pub(super) fn eval_py_boolean(node: Node, source: &str) -> bool {
    let op = node
        .child_by_field_name("operator")
        .map(|n| &source[n.start_byte()..n.end_byte()])
        .unwrap_or("");
    let mut cursor = node.walk();
    let mut operands = Vec::new();
    for child in node.children(&mut cursor) {
        if child.kind() == "and" || child.kind() == "or" {
            continue;
        }
        if child.is_named() {
            operands.push(child);
        }
    }
    if operands.is_empty() {
        return false;
    }
    match op {
        "and" => operands.iter().all(|o| super::is_py_const_false(*o, source)),
        "or" => operands.iter().all(|o| super::is_py_const_false(*o, source)),
        _ => false,
    }
}

pub(super) fn eval_py_comparison(node: Node, source: &str) -> Option<bool> {
    let mut cursor = node.walk();
    let mut left = None;
    let mut op = None;
    let mut right = None;
    for child in node.children(&mut cursor) {
        match child.kind() {
            "==" | "!=" | "<" | ">" | "<=" | ">=" => op = Some(child.kind()),
            _ if left.is_none() => left = Some(child),
            _ if right.is_none() => right = Some(child),
            _ => {}
        }
    }
    let (l, r, op) = (left?, right?, op?);
    let lv = py_literal_i64(l, source)?;
    let rv = py_literal_i64(r, source)?;
    cmp_i64(op, lv, rv)
}

fn cmp_i64(op: &str, lv: i64, rv: i64) -> Option<bool> {
    match op {
        "==" => Some(lv == rv),
        "!=" => Some(lv != rv),
        "<" => Some(lv < rv),
        ">" => Some(lv > rv),
        "<=" => Some(lv <= rv),
        ">=" => Some(lv >= rv),
        _ => None,
    }
}

fn py_literal_i64(node: Node, source: &str) -> Option<i64> {
    match node.kind() {
        "integer" => source[node.start_byte()..node.end_byte()].parse().ok(),
        "true" => Some(1),
        "false" => Some(0),
        "none" => Some(0),
        _ => None,
    }
}
