use crate::graph::DependencyGraph;
use crate::parsing::ParsedFile;
use crate::py_metrics::{
    compute_class_metrics, compute_file_metrics, compute_function_metrics, FunctionMetrics,
};
use tree_sitter::Node;

use super::types::UnitMetrics;
use super::{FileScopeMetrics, file_unit_metrics};

pub fn collect_detailed_py(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> Vec<UnitMetrics> {
    let mut units = Vec::new();
    for parsed in parsed_files {
        let fm = compute_file_metrics(parsed);
        let lines = parsed.source.lines().count();
        units.push(file_unit_metrics(
            &parsed.path,
            FileScopeMetrics {
                lines,
                imports: fm.imports,
                statements: fm.statements,
                functions: fm.functions,
                interface_types: fm.interface_types,
                concrete_types: fm.concrete_types,
            },
            graph,
        ));
        collect_detailed_from_node(
            parsed.tree.root_node(),
            &parsed.source,
            &parsed.path.display().to_string(),
            &mut units,
        );
    }
    units
}

fn unit_metrics_from_py_function(file: &str, name: &str, line: usize, m: &FunctionMetrics) -> UnitMetrics {
    let mut u = UnitMetrics::new(file.to_string(), name.to_string(), "function", line);
    u.statements = Some(m.statements);
    u.arguments = Some(m.arguments);
    u.args_positional = Some(m.arguments_positional);
    u.args_keyword_only = Some(m.arguments_keyword_only);
    u.indentation = Some(m.max_indentation);
    u.nested_depth = Some(m.nested_function_depth);
    u.branches = Some(m.branches);
    u.returns = Some(m.returns);
    u.return_values = Some(m.max_return_values);
    u.locals = Some(m.local_variables);
    u.try_block_statements = Some(m.max_try_block_statements);
    u.boolean_parameters = Some(m.boolean_parameters);
    u.annotations = Some(m.decorators);
    u.calls = Some(m.calls);
    u
}

fn walk_detailed_children(node: Node, source: &str, file: &str, units: &mut Vec<UnitMetrics>) {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        collect_detailed_from_node(child, source, file, units);
    }
}

/// Returns `true` if children were already walked (parse-error function body).
fn push_py_function_unit(node: Node, source: &str, file: &str, units: &mut Vec<UnitMetrics>) -> bool {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("?");
    let m = compute_function_metrics(node, source);
    if m.has_error {
        walk_detailed_children(node, source, file, units);
        return true;
    }
    let line = node.start_position().row + 1;
    units.push(unit_metrics_from_py_function(file, name, line, &m));
    false
}

fn push_py_class_unit(node: Node, source: &str, file: &str, units: &mut Vec<UnitMetrics>) {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("?");
    let m = compute_class_metrics(node);
    let mut u = UnitMetrics::new(
        file.to_string(),
        name.to_string(),
        "class",
        node.start_position().row + 1,
    );
    u.methods = Some(m.methods);
    units.push(u);
}

fn collect_detailed_from_node(node: Node, source: &str, file: &str, units: &mut Vec<UnitMetrics>) {
    let skip_walk = match node.kind() {
        "function_definition" | "async_function_definition" => {
            push_py_function_unit(node, source, file, units)
        }
        "class_definition" => {
            push_py_class_unit(node, source, file, units);
            false
        }
        _ => false,
    };
    if !skip_walk {
        walk_detailed_children(node, source, file, units);
    }
}

#[cfg(test)]
pub(crate) fn collect_detailed_from_node_for_test(
    node: Node,
    source: &str,
    file: &str,
    units: &mut Vec<UnitMetrics>,
) {
    collect_detailed_from_node(node, source, file, units);
}

#[cfg(test)]
mod python_coverage {
    use super::*;
    use std::io::Write;

    #[test]
    fn touch_for_coverage() {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "def foo(a, b):\n    return a + b\n\nclass C:\n    def m(self):\n        pass\n").unwrap();
        let parsed = crate::parsing::parse_file(
            &mut crate::parsing::create_parser().unwrap(),
            tmp.path(),
        )
        .unwrap();
        let refs: Vec<&crate::parsing::ParsedFile> = vec![&parsed];
        let units = collect_detailed_py(&refs, None);
        assert!(units.len() >= 3, "expected file + function + class units, got {}", units.len());
        assert!(units.iter().any(|u| u.kind == "function"), "expected a function unit");
        assert!(units.iter().any(|u| u.kind == "class"), "expected a class unit");
    }

    #[test]
    fn collect_detailed_from_node_produces_function_metrics() {
        let source = "def greet(name):\n    print(name)\n    return name\n";
        let mut parser = crate::parsing::create_parser().unwrap();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_node_for_test(tree.root_node(), source, "test.py", &mut units);

        assert_eq!(units.len(), 1);
        let u = &units[0];
        assert_eq!(u.kind, "function");
        assert_eq!(u.name, "greet");
        assert_eq!(u.file, "test.py");
        assert!(u.statements.unwrap() >= 2);
        assert_eq!(u.arguments.unwrap(), 1);
        assert!(u.returns.is_some());
    }

    #[test]
    fn collect_detailed_from_node_produces_class_metrics() {
        let source = "class Dog:\n    def bark(self):\n        pass\n    def sit(self):\n        pass\n";
        let mut parser = crate::parsing::create_parser().unwrap();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_node_for_test(tree.root_node(), source, "test.py", &mut units);

        let class_units: Vec<_> = units.iter().filter(|u| u.kind == "class").collect();
        assert_eq!(class_units.len(), 1);
        assert_eq!(class_units[0].name, "Dog");
        assert_eq!(class_units[0].methods.unwrap(), 2);

        assert_eq!(units.iter().filter(|u| u.kind == "function").count(), 2);
    }

    #[test]
    fn unit_metrics_from_py_function_fields() {
        let source = "def add(a, b=0):\n    x = a + b\n    return x\n";
        let mut parser = crate::parsing::create_parser().unwrap();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_node_for_test(tree.root_node(), source, "test.py", &mut units);

        let u = &units[0];
        assert_eq!(u.kind, "function");
        assert!(u.locals.is_some());
        assert!(u.branches.is_some());
        assert!(u.calls.is_some());
        assert!(u.indentation.is_some());
        assert!(u.nested_depth.is_some());
        assert!(u.boolean_parameters.is_some());
        assert!(u.annotations.is_some());
        assert!(u.try_block_statements.is_some());
        assert!(u.return_values.is_some());
    }

    #[test]
    fn walk_detailed_children_traverses_nested() {
        let source = "if True:\n    def inner(x):\n        pass\n";
        let mut parser = crate::parsing::create_parser().unwrap();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_node_for_test(tree.root_node(), source, "test.py", &mut units);
        assert!(
            units.iter().any(|u| u.name == "inner"),
            "walk_detailed_children should find nested function"
        );
    }

    #[test]
    fn push_py_function_unit_and_push_py_class_unit_names() {
        let source = "class Outer:\n    def method_a(self, flag: bool):\n        return 1\n\ndef standalone():\n    pass\n";
        let mut parser = crate::parsing::create_parser().unwrap();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_node_for_test(tree.root_node(), source, "f.py", &mut units);

        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Outer"), "push_py_class_unit should emit class");
        assert!(names.contains(&"method_a"), "push_py_function_unit should emit method");
        assert!(names.contains(&"standalone"), "push_py_function_unit should emit standalone fn");
    }
}
