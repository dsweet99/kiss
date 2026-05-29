use super::detection::{is_abstract_method, is_protocol_class};
use super::CodeDefinition;
use crate::units::{CodeUnitKind, get_child_by_field};
use std::collections::HashSet;
use std::path::Path;
use tree_sitter::Node;

#[path = "collect_imports.rs"]
mod collect_imports;
#[allow(unused_imports)]
pub(crate) use collect_imports::{
    collect_call_target, collect_py_import_alias_map, collect_py_import_names_for_refs,
    collect_raises_type_args, collect_type_refs, expand_witness_refs_via_import_aliases,
    extract_import_from_binding, extract_import_statement_modules,
};

pub(crate) fn try_add_py_def(
    node: Node,
    source: &str,
    file: &Path,
    defs: &mut Vec<CodeDefinition>,
    kind: CodeUnitKind,
    containing_class: Option<String>,
    include_private_module_funcs: bool,
) {
    if let Some(name) = get_child_by_field(node, "name", source)
        && (!name.starts_with('_') || name == "__init__" || include_private_module_funcs)
        && !name.starts_with("test_")
    {
        defs.push(CodeDefinition {
            name,
            kind,
            file: file.to_path_buf(),
            line: node.start_position().row + 1,
            end_line: node.end_position().row + 1,
            containing_class,
        });
    }
}

const OI_PROTOCOL_HEADER_LINES: usize = 3;

pub(crate) fn is_py_oi_protocol_stub_path(file: &Path) -> bool {
    let comps: Vec<&str> = file
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let in_oi = comps.windows(2).any(|p| p[0] == "base" && p[1] == "oi");
    in_oi
        && file
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| matches!(n, "interfaces.py" | "evaluate.py"))
}

pub(crate) fn is_dunder_main_guard(node: Node, source: &str) -> bool {
    if node.kind() != "if_statement" {
        return false;
    }
    let Some(cond) = node.child_by_field_name("condition") else {
        return false;
    };
    let text: String = source[cond.start_byte()..cond.end_byte()]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    text == "__name__==\"__main__\"" || text == "__name__=='__main__'"
}

pub(crate) fn collect_definitions(
    node: Node,
    source: &str,
    file: &Path,
    defs: &mut Vec<CodeDefinition>,
    include_private_module_funcs: bool,
    class_name: Option<&str>,
) {
    let inside_class = class_name.is_some();
    match node.kind() {
        "if_statement" if is_dunder_main_guard(node, source) => {}
        "function_definition" | "async_function_definition" if is_abstract_method(node, source) => {
        }
        "function_definition" | "async_function_definition" => {
            let kind = if inside_class {
                CodeUnitKind::Method
            } else {
                CodeUnitKind::Function
            };
            try_add_py_def(
                node,
                source,
                file,
                defs,
                kind,
                class_name.map(String::from),
                include_private_module_funcs && !inside_class,
            );
        }
        "class_definition" if is_protocol_class(node, source) => {
            // OI Protocol stubs are runtime-covered via submodule imports but excluded from defs (g11).
            if is_py_oi_protocol_stub_path(file) {
                try_add_py_def(node, source, file, defs, CodeUnitKind::Class, None, false);
                if let Some(d) = defs.last_mut() {
                    d.end_line = d
                        .line
                        .saturating_add(OI_PROTOCOL_HEADER_LINES.saturating_sub(1))
                        .min(node.end_position().row + 1);
                }
            }
        }
        "class_definition" => {
            try_add_py_def(
                node,
                source,
                file,
                defs,
                CodeUnitKind::Class,
                None,
                false,
            );
            let name = get_child_by_field(node, "name", source);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_definitions(
                    child,
                    source,
                    file,
                    defs,
                    include_private_module_funcs,
                    name.as_deref(),
                );
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_definitions(
                    child,
                    source,
                    file,
                    defs,
                    include_private_module_funcs,
                    class_name,
                );
            }
        }
    }
}

pub(crate) fn insert_identifier(node: Node, source: &str, refs: &mut HashSet<String>) {
    refs.insert(source[node.start_byte()..node.end_byte()].to_string());
}

pub(crate) fn collect_usage_refs_in_scope(node: Node, source: &str, refs: &mut HashSet<String>) {
    collect_usage_refs_in_scope_with_mode(node, source, refs, UsageRefMode::Gate);
}

#[derive(Copy, Clone)]
pub(crate) enum UsageRefMode {
    Gate,
    /// `kiss-coverage-map`: call sites only — no type annotations or decorators.
    CalibrationWitness,
}

pub(crate) fn collect_usage_refs_in_scope_with_mode(
    node: Node,
    source: &str,
    refs: &mut HashSet<String>,
    mode: UsageRefMode,
) {
    match node.kind() {
        "call" => {
            if let Some(func) = node.child_by_field_name("function") {
                collect_call_target(func, source, refs);
            }
            if matches!(mode, UsageRefMode::CalibrationWitness) {
                collect_raises_type_args(node, source, refs);
            }
        }
        "type" if matches!(mode, UsageRefMode::Gate) => {
            collect_type_refs(node, source, refs);
        }
        "decorator" if matches!(mode, UsageRefMode::Gate) => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier"
                    || child.kind() == "attribute"
                    || child.kind() == "call"
                {
                    collect_call_target(child, source, refs);
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_usage_refs_in_scope_with_mode(child, source, refs, mode);
    }
}

#[cfg(test)]
pub(crate) fn collect_class_test_methods(
    class_body: Node,
    source: &str,
    class_prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
) {
    collect_class_test_methods_with_mode(class_body, source, class_prefix, out, UsageRefMode::Gate);
}

pub(crate) fn collect_test_functions_with_refs(
    node: Node,
    source: &str,
    prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
) {
    collect_test_functions_with_refs_mode(node, source, prefix, out, UsageRefMode::Gate);
}

pub(crate) fn collect_test_functions_with_refs_for_coverage_map(
    node: Node,
    source: &str,
    prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
) {
    collect_test_functions_with_refs_mode(
        node,
        source,
        prefix,
        out,
        UsageRefMode::CalibrationWitness,
    );
}

pub(crate) fn collect_test_functions_with_refs_mode(
    node: Node,
    source: &str,
    prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
    witness_mode: UsageRefMode,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = get_child_by_field(node, "name", source).unwrap_or_default();
            if name.starts_with("test_") {
                let mut refs = HashSet::new();
                if let Some(body) = node.child_by_field_name("body") {
                    collect_usage_refs_in_scope_with_mode(body, source, &mut refs, witness_mode);
                }
                let test_id = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}::{name}")
                };
                out.push((test_id, refs));
            }
        }
        "class_definition" => {
            let class_name = get_child_by_field(node, "name", source).unwrap_or_default();
            if super::detection::is_calibration_test_class(node, source) {
                let class_prefix = if prefix.is_empty() {
                    class_name
                } else {
                    format!("{prefix}::{class_name}")
                };
                if let Some(body) = node.child_by_field_name("body") {
                    collect_class_test_methods_with_mode(
                        body,
                        source,
                        &class_prefix,
                        out,
                        witness_mode,
                    );
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_test_functions_with_refs_mode(child, source, prefix, out, witness_mode);
            }
        }
    }
}

pub(crate) fn collect_class_test_methods_with_mode(
    class_body: Node,
    source: &str,
    class_prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
    witness_mode: UsageRefMode,
) {
    let mut cursor = class_body.walk();
    for child in class_body.children(&mut cursor) {
        if child.kind() != "function_definition" && child.kind() != "async_function_definition" {
            continue;
        }
        let meth_name = get_child_by_field(child, "name", source).unwrap_or_default();
        if !meth_name.starts_with("test_") {
            continue;
        }
        let mut refs = HashSet::new();
        if let Some(meth_body) = child.child_by_field_name("body") {
            collect_usage_refs_in_scope_with_mode(meth_body, source, &mut refs, witness_mode);
        }
        let test_id = format!("{class_prefix}::{meth_name}");
        out.push((test_id, refs));
    }
}
