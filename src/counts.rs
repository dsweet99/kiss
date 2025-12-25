
use crate::config::Config;
use crate::parsing::ParsedFile;
use crate::py_metrics::{
    compute_class_metrics_with_source, compute_file_metrics, compute_function_metrics,
    FileMetrics, FunctionMetrics,
};
use crate::violation::{Violation, ViolationBuilder};
use std::path::Path;
use tree_sitter::Node;

pub use crate::py_metrics::{
    compute_class_metrics, compute_file_metrics as get_file_metrics,
    compute_function_metrics as get_function_metrics, ClassMetrics as PyClassMetrics,
    FileMetrics as PyFileMetrics, FunctionMetrics as PyFunctionMetrics,
};
pub use crate::violation::{Violation as PyViolation, ViolationBuilder as PyViolationBuilder};

#[must_use]
pub fn analyze_file(parsed: &ParsedFile, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let file = &parsed.path;
    let fname = file.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

    let file_metrics = compute_file_metrics(parsed);
    check_file_metrics(&file_metrics, file, &fname, config, &mut violations);

    analyze_node(parsed.tree.root_node(), &parsed.source, file, &mut violations, false, config);
    violations
}

fn check_file_metrics(m: &FileMetrics, file: &Path, fname: &str, cfg: &Config, v: &mut Vec<Violation>) {
    if m.lines > cfg.lines_per_file {
        v.push(violation(file, 1, fname).metric("lines_per_file").value(m.lines).threshold(cfg.lines_per_file)
            .message(format!("File has {} lines (threshold: {})", m.lines, cfg.lines_per_file))
            .suggestion("Split into multiple modules with focused responsibilities.").build());
    }
    if m.classes > cfg.classes_per_file {
        v.push(violation(file, 1, fname).metric("classes_per_file").value(m.classes).threshold(cfg.classes_per_file)
            .message(format!("File has {} classes (threshold: {})", m.classes, cfg.classes_per_file))
            .suggestion("Consider splitting into separate modules.").build());
    }
    if m.imports > cfg.imports_per_file {
        v.push(violation(file, 1, fname).metric("imports_per_file").value(m.imports).threshold(cfg.imports_per_file)
            .message(format!("File has {} imports (threshold: {})", m.imports, cfg.imports_per_file))
            .suggestion("Consider reducing dependencies or splitting the module.").build());
    }
}

fn violation(file: &Path, line: usize, name: &str) -> ViolationBuilder {
    Violation::builder(file).line(line).unit_name(name)
}

enum Recursion { Skip, Continue(bool) }

fn analyze_node(node: Node, source: &str, file: &Path, violations: &mut Vec<Violation>, inside_class: bool, config: &Config) {
    let recursion = match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = node.child_by_field_name("name").and_then(|n| n.utf8_text(source.as_bytes()).ok()).unwrap_or("<anonymous>");
            let line = node.start_position().row + 1;
            let m = compute_function_metrics(node, source);
            check_function_metrics(&m, file, line, name, inside_class, config, violations);
            Recursion::Skip
        }
        "class_definition" => {
            analyze_class_node(node, source, file, violations, config);
            Recursion::Skip
        }
        _ => Recursion::Continue(inside_class),
    };
    if let Recursion::Continue(ctx) = recursion {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            analyze_node(child, source, file, violations, ctx, config);
        }
    }
}

fn check_function_metrics(m: &FunctionMetrics, file: &Path, line: usize, name: &str, inside_class: bool, cfg: &Config, v: &mut Vec<Violation>) {
    let ut = if inside_class { "Method" } else { "Function" };
    macro_rules! chk {
        ($mf:ident, $cf:ident, $metric:literal, $label:literal, $sug:literal) => {
            if m.$mf > cfg.$cf {
                v.push(violation(file, line, name).metric($metric).value(m.$mf).threshold(cfg.$cf)
                    .message(format!("{} '{}' has {} {} (threshold: {})", ut, name, m.$mf, $label, cfg.$cf))
                    .suggestion($sug).build());
            }
        };
    }
    chk!(statements, statements_per_function, "statements_per_function", "statements", "Break into smaller, focused functions.");
    if !inside_class && m.arguments_positional > cfg.arguments_positional {
        v.push(violation(file, line, name).metric("positional_args").value(m.arguments_positional).threshold(cfg.arguments_positional)
            .message(format!("Function '{}' has {} positional arguments (threshold: {})", name, m.arguments_positional, cfg.arguments_positional))
            .suggestion("Consider using keyword-only arguments, a config object, or the builder pattern.").build());
    }
    chk!(arguments_keyword_only, arguments_keyword_only, "keyword_only_args", "keyword-only arguments", "Consider grouping related parameters into a config object.");
    chk!(max_indentation, max_indentation_depth, "max_indentation", "indentation depth", "Extract nested logic into helper functions or use early returns.");
    chk!(nested_function_depth, nested_function_depth, "nested_function_depth", "nested functions", "Move nested functions to module level or use classes.");
    chk!(branches, branches_per_function, "branches_per_function", "branches", "Consider using polymorphism, lookup tables, or the strategy pattern.");
    chk!(local_variables, local_variables_per_function, "local_variables", "local variables", "Extract related variables into a data class or split the function.");
    chk!(max_try_block_statements, statements_per_try_block, "statements_per_try_block", "statements in try block", "Keep try blocks narrow: wrap only the code that can raise the specific exception.");
    chk!(boolean_parameters, boolean_parameters, "boolean_parameters", "boolean parameters", "Use keyword-only arguments, an enum, or separate functions instead of boolean flags.");
    chk!(decorators, decorators_per_function, "decorators_per_function", "decorators", "Consider consolidating decorators or simplifying the function's responsibilities.");
}

fn analyze_class_node(node: Node, source: &str, file: &Path, violations: &mut Vec<Violation>, config: &Config) {
    let name = node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("<anonymous>");
    let line = node.start_position().row + 1;
    let m = compute_class_metrics_with_source(node, source);

    if m.methods > config.methods_per_class {
        violations.push(violation(file, line, name).metric("methods_per_class").value(m.methods).threshold(config.methods_per_class)
            .message(format!("Class '{}' has {} methods (threshold: {})", name, m.methods, config.methods_per_class))
            .suggestion("Consider extracting groups of related methods into separate classes.").build());
    }
    let lcom_pct = (m.lcom * 100.0) as usize;
    if m.methods > 20 && lcom_pct > config.lcom {
        violations.push(violation(file, line, name).metric("lcom").value(lcom_pct).threshold(config.lcom)
            .message(format!("Class '{}' may be a God Class: {} methods with {}% LCOM (threshold: {} methods, {}% LCOM)", name, m.methods, lcom_pct, 20, config.lcom))
            .suggestion("Consider splitting into multiple focused classes with cohesive responsibilities.").build());
    }
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            analyze_node(child, source, file, violations, true, config);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use std::io::Write;
    use std::path::PathBuf;

    fn parse_source(code: &str) -> ParsedFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{code}").unwrap();
        let mut parser = create_parser().unwrap();
        parse_file(&mut parser, tmp.path()).unwrap()
    }

    #[test]
    fn test_analyze_file_no_violations() {
        let parsed = parse_source("def f(): pass");
        let violations = analyze_file(&parsed, &Config::default());
        assert!(violations.is_empty());
    }

    #[test]
    fn test_analyze_file_with_violation() {
        let parsed = parse_source("def f(a,b,c,d,e,f,g,h,i,j): pass");
        let mut config = Config::default();
        config.arguments_positional = 5;
        let violations = analyze_file(&parsed, &config);
        assert!(!violations.is_empty());
    }

    #[test]
    fn test_violation_builder() {
        let v = violation(&PathBuf::from("f.py"), 1, "n")
            .metric("m").value(10).threshold(5).message("msg").suggestion("sug").build();
        assert_eq!(v.value, 10);
        assert_eq!(v.threshold, 5);
    }

    #[test]
    fn test_analyze_node() {
        let parsed = parse_source("def f(): pass\nclass C: pass");
        let mut viols = Vec::new();
        analyze_node(parsed.tree.root_node(), &parsed.source, &parsed.path, &mut viols, false, &Config::default());
        assert!(viols.is_empty());
    }

    #[test]
    fn test_analyze_class_node() {
        let parsed = parse_source("class C:\n    def m(self): pass");
        let mut viols = Vec::new();
        let cls = parsed.tree.root_node().child(0).unwrap();
        analyze_class_node(cls, &parsed.source, &parsed.path, &mut viols, &Config::default());
        assert!(viols.is_empty());
    }

    #[test]
    fn test_check_file_metrics() {
        let m = FileMetrics { lines: 1000, classes: 20, imports: 50 };
        let mut cfg = Config::default();
        cfg.lines_per_file = 500;
        cfg.classes_per_file = 10;
        cfg.imports_per_file = 30;
        let mut viols = Vec::new();
        check_file_metrics(&m, Path::new("t.py"), "t.py", &cfg, &mut viols);
        assert_eq!(viols.len(), 3);
    }

    #[test]
    fn test_analyze_node_function() {
        let parsed = parse_source("def f(a): x = 1");
        let func = parsed.tree.root_node().child(0).unwrap();
        let mut viols = Vec::new();
        analyze_node(func, &parsed.source, &parsed.path, &mut viols, false, &Config::default());
        assert!(viols.is_empty());
    }

    #[test]
    fn test_check_function_metrics() {
        let m = FunctionMetrics { statements: 100, arguments: 0, arguments_positional: 10, arguments_keyword_only: 10, max_indentation: 10, nested_function_depth: 5, returns: 0, branches: 20, local_variables: 30, max_try_block_statements: 0, boolean_parameters: 0, decorators: 0 };
        let cfg = Config { statements_per_function: 50, arguments_positional: 5, arguments_keyword_only: 5, max_indentation_depth: 5, nested_function_depth: 2, branches_per_function: 10, local_variables_per_function: 15, ..Default::default() };
        let mut viols = Vec::new();
        check_function_metrics(&m, Path::new("t.py"), 1, "f", false, &cfg, &mut viols);
        assert!(viols.len() >= 5);
    }

    #[test]
    fn test_recursion_enum() {
        let skip = Recursion::Skip;
        let cont = Recursion::Continue(true);
        assert!(matches!(skip, Recursion::Skip));
        assert!(matches!(cont, Recursion::Continue(true)));
    }
}
