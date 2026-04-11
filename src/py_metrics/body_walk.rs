use std::collections::HashSet;
use tree_sitter::Node;

use super::indent_scope::{is_nested_scope_boundary, next_indent_depth};
use super::locals::update_local_vars;
use super::returns::count_return_values;
use super::statements::is_statement;

#[derive(Default)]
pub(crate) struct BodyAgg {
    pub(crate) statements: usize,
    pub(crate) max_indentation: usize,
    pub(crate) branches: usize,
    pub(crate) returns: usize,
    pub(crate) calls: usize,
    pub(crate) max_try_block_statements: usize,
    pub(crate) max_return_values: usize,
    pub(crate) local_vars: HashSet<String>,
}

pub(crate) struct BodySummary {
    pub(crate) statements: usize,
    pub(crate) max_indentation: usize,
    pub(crate) branches: usize,
    pub(crate) local_variables: usize,
    pub(crate) returns: usize,
    pub(crate) calls: usize,
    pub(crate) max_try_block_statements: usize,
    pub(crate) max_return_values: usize,
}

pub(crate) fn analyze_body(body: Node, source: &str) -> BodySummary {
    let mut agg = BodyAgg::default();
    // Start at indentation depth 1 (function body baseline).
    let _ = walk_body(body, source, 1, &mut agg);
    BodySummary {
        statements: agg.statements,
        max_indentation: agg.max_indentation,
        branches: agg.branches,
        local_variables: agg.local_vars.len(),
        returns: agg.returns,
        calls: agg.calls,
        max_try_block_statements: agg.max_try_block_statements,
        max_return_values: agg.max_return_values,
    }
}

// Returns statement count for this subtree (including this node if it is a statement).
pub(crate) fn walk_body(node: Node, source: &str, current_depth: usize, agg: &mut BodyAgg) -> usize {
    let kind = node.kind();
    let is_nested_scope = is_nested_scope_boundary(kind);
    let new_depth = if is_nested_scope {
        current_depth
    } else {
        next_indent_depth(kind, current_depth)
    };
    agg.max_indentation = agg.max_indentation.max(new_depth);

    update_local_vars(node, source, &mut agg.local_vars);
    update_body_counts(node, agg);
    update_return_counts(node, agg);

    let try_body_range = try_body_byte_range(node);

    let mut subtree_stmt_count = usize::from(is_statement(kind));
    if is_nested_scope {
        return subtree_stmt_count;
    }
    let mut try_body_stmt_count: Option<usize> = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let child_stmts = walk_body(child, source, new_depth, agg);
        subtree_stmt_count += child_stmts;
        if is_try_body(child, try_body_range) {
            try_body_stmt_count = Some(child_stmts);
        }
    }
    update_try_block_statements(node, try_body_stmt_count, agg);
    subtree_stmt_count
}

pub(crate) fn update_body_counts(node: Node, agg: &mut BodyAgg) {
    if is_statement(node.kind()) {
        agg.statements += 1;
    }
    if matches!(node.kind(), "if_statement" | "elif_clause" | "case_clause") {
        agg.branches += 1;
    }
    if node.kind() == "call" {
        agg.calls += 1;
    }
}

pub(crate) fn update_return_counts(node: Node, agg: &mut BodyAgg) {
    if node.kind() == "return_statement" {
        agg.returns += 1;
        agg.max_return_values = agg.max_return_values.max(count_return_values(node));
    }
}

pub(crate) fn try_body_byte_range(node: Node) -> Option<(usize, usize)> {
    (node.kind() == "try_statement")
        .then(|| node.child_by_field_name("body"))
        .flatten()
        .map(|b| (b.start_byte(), b.end_byte()))
}

pub(crate) fn is_try_body(child: Node, try_body_range: Option<(usize, usize)>) -> bool {
    if let Some((sb, eb)) = try_body_range {
        child.start_byte() == sb && child.end_byte() == eb
    } else {
        false
    }
}

pub(crate) fn update_try_block_statements(
    node: Node,
    try_body_stmt_count: Option<usize>,
    agg: &mut BodyAgg,
) {
    if node.kind() == "try_statement"
        && let Some(body_stmts) = try_body_stmt_count
    {
        agg.max_try_block_statements = agg.max_try_block_statements.max(body_stmts);
    }
}
