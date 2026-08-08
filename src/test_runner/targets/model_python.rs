//! Python source model for `kiss test` targets (AST only; no lexical fallback).

use std::path::Path;

use tree_sitter::{Node, Parser};

use super::model::{DirectTestDef, NamedDefinition, SourceModel, byte_span_to_lines};
use kiss::Language;

pub(super) fn build_python_model(
    path: &Path,
    content: String,
    line_count: u32,
) -> Result<SourceModel, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("tree-sitter Python grammar should be available");
    let tree = parser
        .parse(&content, None)
        .ok_or_else(|| format!("failed to parse Python source {}", path.display()))?;
    let root = tree.root_node();
    if root.has_error() {
        return Err(format!(
            "failed to parse Python source {}: syntax error",
            path.display()
        ));
    }
    let mut definitions = Vec::new();
    let mut direct_tests = Vec::new();
    walk_python(root, &content, None, &mut definitions, &mut direct_tests);
    Ok(SourceModel {
        path: path.to_path_buf(),
        language: Language::Python,
        direct_tests,
        definitions,
        line_count,
    })
}

fn walk_python(
    node: Node<'_>,
    content: &str,
    owner: Option<&str>,
    definitions: &mut Vec<NamedDefinition>,
    direct_tests: &mut Vec<DirectTestDef>,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            push_python_function(node, content, owner, definitions, direct_tests);
            return;
        }
        "class_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let class_name = content[name_node.start_byte()..name_node.end_byte()].to_string();
                let (start_line, end_line) =
                    byte_span_to_lines(content, node.start_byte(), node.end_byte());
                definitions.push(NamedDefinition {
                    name: class_name.clone(),
                    member: None,
                    start_line,
                    end_line,
                    is_unit_test: false,
                    test_selector: None,
                });
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    walk_python(
                        child,
                        content,
                        Some(class_name.as_str()),
                        definitions,
                        direct_tests,
                    );
                }
                return;
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_python(child, content, owner, definitions, direct_tests);
    }
}

fn push_python_function(
    node: Node<'_>,
    content: &str,
    owner: Option<&str>,
    definitions: &mut Vec<NamedDefinition>,
    direct_tests: &mut Vec<DirectTestDef>,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = content[name_node.start_byte()..name_node.end_byte()].to_string();
    let (start_line, end_line) = byte_span_to_lines(content, node.start_byte(), node.end_byte());
    let is_test = looks_like_python_test(&name, owner);
    let (def_name, def_member) = match owner {
        Some(owner) => (owner.to_string(), Some(name.clone())),
        None => (name.clone(), None),
    };
    if is_test {
        direct_tests.push(DirectTestDef {
            selector: String::new(),
            name: name.clone(),
            owner: owner.map(str::to_string),
            start_line,
            end_line,
        });
    }
    definitions.push(NamedDefinition {
        name: def_name,
        member: def_member,
        start_line,
        end_line,
        is_unit_test: is_test,
        test_selector: None,
    });
}

fn looks_like_python_test(name: &str, owner: Option<&str>) -> bool {
    name.starts_with("test_")
        || owner.is_some_and(|owner| owner.starts_with("Test") && name.starts_with("test"))
}

/// Replace provisional selectors with stable pytest nodeids when collection succeeds.
pub(super) fn attach_python_nodeids(
    model: &mut SourceModel,
    nodeids: &[String],
    repo_relative: &str,
) {
    let pending = std::mem::take(&mut model.direct_tests);
    for test in pending {
        let matches =
            match_python_nodeids(nodeids, repo_relative, &test.name, test.owner.as_deref());
        if matches.is_empty() {
            continue;
        }
        for def in &mut model.definitions {
            let matches_def = match test.owner.as_deref() {
                Some(owner) => {
                    def.name == owner && def.member.as_deref() == Some(test.name.as_str())
                }
                None => def.name == test.name && def.member.is_none(),
            };
            if matches_def && def.is_unit_test {
                def.test_selector = Some(matches[0].clone());
            }
        }
        for nodeid in matches {
            let mut attached = test.clone();
            attached.selector = nodeid;
            model.direct_tests.push(attached);
        }
    }
}

fn match_python_nodeids(
    nodeids: &[String],
    repo_relative: &str,
    name: &str,
    owner: Option<&str>,
) -> Vec<String> {
    let base = match owner {
        Some(owner) => format!("{repo_relative}::{owner}::{name}"),
        None => format!("{repo_relative}::{name}"),
    };
    let base_param = format!("{base}[");
    let suffix = match owner {
        Some(owner) => format!("::{owner}::{name}"),
        None => format!("::{name}"),
    };
    let suffix_param = format!("{suffix}[");
    nodeids
        .iter()
        .filter(|nodeid| {
            *nodeid == &base
                || nodeid.starts_with(&base_param)
                || (nodeid.starts_with(repo_relative)
                    && (nodeid.ends_with(&suffix) || nodeid.contains(&suffix_param)))
        })
        .cloned()
        .collect()
}
