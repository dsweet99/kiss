use super::coverage_expand_paths::{is_py_contrib_base_void_partition, unquote_py_string};
use crate::test_refs::detection::is_python_test_file;
use crate::parsing::ParsedFile;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tree_sitter::Node;

/// Void-partition files attested by dynamic-dispatch literals in tests (`patch`, `setattr`,
/// `import_module`, `pytest.raises` targets, `.py` path strings). Normal call witnesses do not
/// credit void partitions (prevents hub inflation).
/// Per void-partition file: empty set = path-literal attests any strict-covered def; non-empty =
/// only defs whose names appear in patch/raises targets may be strict-covered.
#[allow(dead_code)]
pub(crate) fn build_py_void_dispatch_attestation(
    parsed_files: &[&ParsedFile],
    module_suffixes: &HashMap<PathBuf, String>,
) -> HashMap<PathBuf, HashSet<String>> {
    let mut dotted = HashSet::new();
    let mut raises_paths = HashSet::new();
    let mut path_literals = HashSet::new();
    for parsed in parsed_files {
        if !is_python_test_file(parsed) {
            continue;
        }
        collect_py_dynamic_dispatch_literals(
            parsed.tree.root_node(),
            &parsed.source,
            &mut dotted,
        );
        collect_py_raises_dotted_paths(
            parsed.tree.root_node(),
            &parsed.source,
            &mut raises_paths,
        );
        super::coverage_expand_paths::collect_py_path_string_literals(
            parsed.tree.root_node(),
            &parsed.source,
            &mut path_literals,
        );
    }
    let mut attested: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    for lit in path_literals {
        for path in module_suffixes.keys() {
            if is_py_contrib_base_void_partition(path) && path.ends_with(&lit) {
                attested.entry(path.clone()).or_default();
            }
        }
    }
    for d in dotted {
        for path in void_files_for_dotted_path(&d, module_suffixes) {
            if let Some(name) = d.rsplit('.').next() {
                attested.entry(path).or_default().insert(name.to_string());
            }
        }
    }
    for d in raises_paths {
        for path in void_files_for_dotted_path(&d, module_suffixes) {
            if let Some(name) = d.rsplit('.').next() {
                attested.entry(path).or_default().insert(name.to_string());
            }
        }
    }
    attested
}

pub(crate) fn collect_py_raises_dotted_paths(node: Node, source: &str, out: &mut HashSet<String>) {
    if node.kind() == "call"
        && let Some(func) = node.child_by_field_name("function")
        && is_raises_context_call(func, source)
        && let Some(dotted) = exception_path_from_raises_call(node, source)
    {
        out.insert(dotted);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_py_raises_dotted_paths(child, source, out);
    }
}

pub(crate) fn void_files_for_dotted_path(
    dotted: &str,
    module_suffixes: &HashMap<PathBuf, String>,
) -> HashSet<PathBuf> {
    let segments: Vec<&str> = dotted.split('.').collect();
    let mut files = HashSet::new();
    for len in (1..=segments.len()).rev() {
        let candidate = segments[..len].join(".");
        for (path, suffix) in module_suffixes {
            if is_py_contrib_base_void_partition(path)
                && (suffix == &candidate || suffix.ends_with(&format!(".{candidate}")))
            {
                files.insert(path.clone());
            }
        }
        if !files.is_empty() {
            break;
        }
    }
    files
}

pub(crate) fn collect_py_dynamic_dispatch_literals(
    node: Node,
    source: &str,
    out: &mut HashSet<String>,
) {
    if node.kind() == "call"
        && let Some(func) = node.child_by_field_name("function")
    {
        if is_patch_setattr_or_import_module_call(func, source) {
            if let Some(lit) = first_string_literal_arg(node, source) {
                out.insert(lit);
            }
        } else if is_raises_context_call(func, source)
            && let Some(dotted) = exception_path_from_raises_call(node, source)
        {
            out.insert(dotted);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_py_dynamic_dispatch_literals(child, source, out);
    }
}

fn is_patch_setattr_or_import_module_call(func: Node, source: &str) -> bool {
    match func.kind() {
        "identifier" => {
            let name = &source[func.start_byte()..func.end_byte()];
            name == "patch" || name == "setattr"
        }
        "attribute" => func
            .child_by_field_name("attribute")
            .is_some_and(|a| {
                let name = &source[a.start_byte()..a.end_byte()];
                name == "patch" || name == "setattr" || name == "import_module"
            }),
        _ => false,
    }
}

fn is_raises_context_call(func: Node, source: &str) -> bool {
    match func.kind() {
        "identifier" => {
            let name = &source[func.start_byte()..func.end_byte()];
            name == "raises" || name == "assertRaises"
        }
        "attribute" => func
            .child_by_field_name("attribute")
            .is_some_and(|a| {
                let name = &source[a.start_byte()..a.end_byte()];
                name == "raises" || name == "assertRaises"
            }),
        _ => false,
    }
}

fn first_string_literal_arg(call: Node, source: &str) -> Option<String> {
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "string" {
            return Some(unquote_py_string(&source[child.start_byte()..child.end_byte()]));
        }
        if child.kind() == "argument" {
            let mut inner = child.walk();
            for sub in child.children(&mut inner) {
                if sub.kind() == "string" {
                    return Some(unquote_py_string(
                        &source[sub.start_byte()..sub.end_byte()],
                    ));
                }
            }
        }
        if child.kind() != "," && child.kind() != "(" && child.kind() != ")" {
            break;
        }
    }
    None
}

fn exception_path_from_raises_call(call: Node, source: &str) -> Option<String> {
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "," || child.kind() == "(" || child.kind() == ")" {
            continue;
        }
        return exception_path_from_expr(child, source);
    }
    None
}

fn exception_path_from_expr(node: Node, source: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = node;
    loop {
        match cur.kind() {
            "identifier" => {
                parts.push(source[cur.start_byte()..cur.end_byte()].to_string());
                parts.reverse();
                return Some(parts.join("."));
            }
            "attribute" => {
                let attr = cur.child_by_field_name("attribute")?;
                let obj = cur.child_by_field_name("object")?;
                parts.push(source[attr.start_byte()..attr.end_byte()].to_string());
                cur = obj;
            }
            _ => return None,
        }
    }
}
