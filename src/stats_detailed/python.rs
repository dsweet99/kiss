use crate::graph::DependencyGraph;
use crate::parsing::ParsedFile;
use crate::py_metrics::{FunctionMetrics, PyWalkAction, compute_file_metrics, walk_py_ast};
use tree_sitter::Node;

use super::types::UnitMetrics;
use super::{FileScopeMetrics, file_unit_metrics};

fn py_import_metric(path: &std::path::Path, imports: usize) -> Option<usize> {
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    (fname != "__init__.py").then_some(imports)
}

#[must_use]
pub fn collect_detailed_py(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> Vec<UnitMetrics> {
    let mut units = Vec::new();
    for &parsed in parsed_files {
        let fm = compute_file_metrics(parsed);
        let lines = parsed.source.lines().count();
        units.push(file_unit_metrics(
            &parsed.path,
            FileScopeMetrics {
                lines,
                imports: py_import_metric(&parsed.path, fm.imports),
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

fn unit_metrics_from_py_function(
    file: &str,
    name: &str,
    line: usize,
    m: &FunctionMetrics,
) -> UnitMetrics {
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

pub(crate) fn collect_detailed_from_node(
    node: Node,
    source: &str,
    file: &str,
    units: &mut Vec<UnitMetrics>,
) {
    walk_py_ast(
        node,
        source,
        &mut |action| match action {
            PyWalkAction::Function(v) => {
                units.push(unit_metrics_from_py_function(
                    file, v.name, v.line, v.metrics,
                ));
            }
            PyWalkAction::Class(v) => {
                let mut u = UnitMetrics::new(file.to_string(), v.name.to_string(), "class", v.line);
                u.methods = Some(v.metrics.methods);
                units.push(u);
            }
        },
        false,
    );
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
    use super::collect_detailed_py;
    use super::*;
    use std::io::Write;

    #[test]
    fn touch_for_coverage() {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(
            tmp,
            "def foo(a, b):\n    return a + b\n\nclass C:\n    def m(self):\n        pass\n"
        )
        .unwrap();
        let parsed =
            crate::parsing::parse_file(&mut crate::parsing::create_parser().unwrap(), tmp.path())
                .unwrap();
        let refs: Vec<&crate::parsing::ParsedFile> = vec![&parsed];
        let units = collect_detailed_py(&refs, None);
        assert!(
            units.len() >= 3,
            "expected file + function + class units, got {}",
            units.len()
        );
        assert!(
            units.iter().any(|u| u.kind == "function"),
            "expected a function unit"
        );
        assert!(
            units.iter().any(|u| u.kind == "class"),
            "expected a class unit"
        );
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
        let source =
            "class Dog:\n    def bark(self):\n        pass\n    def sit(self):\n        pass\n";
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
    fn walk_py_ast_traverses_nested_for_detailed() {
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
            "walk_py_ast should find nested function"
        );
    }

    #[test]
    fn detailed_units_include_class_methods_and_standalone_fn() {
        let source = "class Outer:\n    def method_a(self, flag: bool):\n        return 1\n\ndef standalone():\n    pass\n";
        let mut parser = crate::parsing::create_parser().unwrap();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_node_for_test(tree.root_node(), source, "f.py", &mut units);

        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Outer"), "expected class unit");
        assert!(names.contains(&"method_a"), "expected method unit");
        assert!(
            names.contains(&"standalone"),
            "expected standalone function unit"
        );
    }
}
