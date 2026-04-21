use crate::parsing::ParsedFile;
use crate::units::get_child_by_field;
use std::path::Path;
use tree_sitter::Node;

pub(crate) fn has_python_test_naming(path: &Path) -> bool {
    let is_py = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"));
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            (name.starts_with("test_") && is_py)
                || (name.len() > 8 && name[..name.len() - 3].ends_with("_test") && is_py)
                || name.eq_ignore_ascii_case("conftest.py")
        })
}

#[must_use]
pub fn is_test_file(path: &std::path::Path) -> bool {
    has_python_test_naming(path)
}

pub(crate) fn is_test_framework(name: &str) -> bool {
    name == "pytest"
        || name == "unittest"
        || name.starts_with("pytest.")
        || name.starts_with("unittest.")
}

pub(crate) fn is_test_framework_import_from(child: Node, source: &str) -> bool {
    child
        .child_by_field_name("module_name")
        .map(|m| &source[m.start_byte()..m.end_byte()])
        .is_some_and(is_test_framework)
}

pub(crate) fn contains_test_module_name(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let name = match child.kind() {
            "dotted_name" => Some(&source[child.start_byte()..child.end_byte()]),
            "aliased_import" => child
                .child_by_field_name("name")
                .map(|n| &source[n.start_byte()..n.end_byte()]),
            _ => None,
        };
        if name.is_some_and(|n| n == "pytest" || n == "unittest") {
            return true;
        }
    }
    false
}

pub fn has_test_framework_import(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" if contains_test_module_name(child, source) => return true,
            "import_from_statement" if is_test_framework_import_from(child, source) => return true,
            _ => {}
        }
    }
    false
}

pub(crate) fn has_test_function_or_class(node: Node, source: &str) -> bool {
    match node.kind() {
        "function_definition" | "async_function_definition" if is_test_function(node, source) => {
            true
        }
        "class_definition" if is_test_class(node, source) => true,
        _ => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .any(|child| has_test_function_or_class(child, source))
        }
    }
}

pub fn is_in_test_directory(path: &Path) -> bool {
    use std::ffi::OsStr;
    path.components()
        .any(|c| c.as_os_str() == OsStr::new("tests") || c.as_os_str() == OsStr::new("test"))
}

pub(crate) fn is_python_test_file(parsed: &ParsedFile) -> bool {
    if is_test_file(&parsed.path) || is_in_test_directory(&parsed.path) {
        return true;
    }
    let root = parsed.tree.root_node();
    has_test_framework_import(root, &parsed.source)
        && has_test_function_or_class(root, &parsed.source)
}

pub(crate) fn is_protocol_class(node: Node, source: &str) -> bool {
    let Some(superclasses) = node.child_by_field_name("superclasses") else {
        return false;
    };
    let mut cursor = superclasses.walk();
    for child in superclasses.children(&mut cursor) {
        let text = &source[child.start_byte()..child.end_byte()];
        if text == "Protocol" || text == "typing.Protocol" {
            return true;
        }
    }
    false
}

pub(crate) fn is_abstract_method(node: Node, source: &str) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "decorated_definition" {
        return false;
    }
    let mut cursor = parent.walk();
    parent.children(&mut cursor).any(|child| {
        child.kind() == "decorator"
            && source[child.start_byte()..child.end_byte()].ends_with("abstractmethod")
    })
}

pub(crate) fn is_test_function(node: Node, source: &str) -> bool {
    get_child_by_field(node, "name", source).is_some_and(|n| n.starts_with("test_"))
}

pub(crate) fn is_test_class(node: Node, source: &str) -> bool {
    get_child_by_field(node, "name", source).is_some_and(|n| n.starts_with("Test"))
}
