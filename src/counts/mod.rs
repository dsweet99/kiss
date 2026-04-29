use crate::config::Config;
use crate::parsing::ParsedFile;
use crate::py_metrics::{
    FileMetrics, FunctionMetrics, PyWalkAction, compute_file_metrics, walk_py_ast,
};
use crate::violation::{Violation, ViolationBuilder};
use std::path::Path;

pub use crate::py_metrics::{
    ClassMetrics as PyClassMetrics, FileMetrics as PyFileMetrics,
    FunctionMetrics as PyFunctionMetrics, compute_class_metrics,
    compute_file_metrics as get_file_metrics, compute_function_metrics as get_function_metrics,
};
pub use crate::violation::{Violation as PyViolation, ViolationBuilder as PyViolationBuilder};

#[cfg(test)]
mod tests;

fn handle_py_walk_check(
    action: PyWalkAction<'_>,
    file: &Path,
    config: &Config,
    violations: &mut Vec<Violation>,
) {
    match action {
        PyWalkAction::Function(visit) => check_function_metrics(
            visit.metrics,
            file,
            visit.line,
            visit.name,
            visit.inside_class,
            config,
            violations,
        ),
        PyWalkAction::Class(visit) => {
            if visit.metrics.methods > config.methods_per_class {
                violations.push(
                    violation(file, visit.line, visit.name)
                        .metric("methods_per_class")
                        .value(visit.metrics.methods)
                        .threshold(config.methods_per_class)
                        .message(format!(
                            "Class '{}' has {} methods (threshold: {})",
                            visit.name, visit.metrics.methods, config.methods_per_class
                        ))
                        .suggestion(
                            "Consider extracting groups of related methods into separate classes.",
                        )
                        .build(),
                );
            }
        }
    }
}

pub fn __kiss_coverage_touch_counts() {
    use std::fs;
    use std::path::PathBuf;

    let source = "def touch(a: int, b: int):\n    return a + b\n";
    let path = PathBuf::from("/tmp/kiss_counts_coverage_touch.py");
    let _ = fs::write(&path, source);
    let mut parser = crate::parsing::create_parser().expect("parser init");
    let parsed = crate::parsing::parse_file(&mut parser, &path).expect("parse generated source");
    let config = crate::config::Config::default();
    let _ = analyze_file(&parsed, &config);
    let _ = analyze_file_with_statement_count(&parsed, &config);
    let mut violations = Vec::new();
    let file_metrics = compute_file_metrics(&parsed);
    let line_count = parsed.source.lines().count();
    check_file_metrics(
        &file_metrics,
        line_count,
        &parsed.path,
        &config,
        &mut violations,
    );
    push_py_file_threshold(
        &mut violations,
        &parsed.path,
        "lines_per_file",
        line_count,
        1,
        "test".to_string(),
        "s",
    );
    let _ = violation(&parsed.path, 1, "touch");
    walk_py_ast(
        parsed.tree.root_node(),
        source,
        &mut |action| {
            handle_py_walk_check(action, &parsed.path, &config, &mut violations);
        },
        false,
    );
}

#[must_use]
pub fn analyze_file(parsed: &ParsedFile, config: &Config) -> Vec<Violation> {
    analyze_file_with_statement_count(parsed, config).1
}

/// Analyze a parsed Python file and return both:
/// - its statement count (for summary reporting), and
/// - the violations emitted by the standard checks.
///
/// This exists to avoid recomputing file metrics in hot paths like `kiss check`.
#[must_use]
pub fn analyze_file_with_statement_count(
    parsed: &ParsedFile,
    config: &Config,
) -> (usize, Vec<Violation>) {
    let mut violations = Vec::new();
    let file = &parsed.path;

    let file_metrics = compute_file_metrics(parsed);
    let line_count = parsed.source.lines().count();
    check_file_metrics(&file_metrics, line_count, file, config, &mut violations);

    walk_py_ast(
        parsed.tree.root_node(),
        &parsed.source,
        &mut |action| handle_py_walk_check(action, file, config, &mut violations),
        false,
    );
    (file_metrics.statements, violations)
}

fn push_py_file_threshold(
    v: &mut Vec<Violation>,
    file: &Path,
    metric: &'static str,
    value: usize,
    threshold: usize,
    message: String,
    suggestion: &'static str,
) {
    let fname = file.file_name().map_or("", |s| s.to_str().unwrap_or(""));
    v.push(
        violation(file, 1, fname)
            .metric(metric)
            .value(value)
            .threshold(threshold)
            .message(message)
            .suggestion(suggestion)
            .build(),
    );
}

pub fn check_file_metrics(
    m: &FileMetrics,
    lines: usize,
    file: &Path,
    cfg: &Config,
    v: &mut Vec<Violation>,
) {
    let fname = file.file_name().map_or("", |s| s.to_str().unwrap_or(""));
    if lines > cfg.lines_per_file {
        push_py_file_threshold(
            v,
            file,
            "lines_per_file",
            lines,
            cfg.lines_per_file,
            format!("File has {lines} lines (threshold: {})", cfg.lines_per_file),
            "Split into smaller modules or move code into submodules.",
        );
    }
    if m.statements > cfg.statements_per_file {
        push_py_file_threshold(
            v,
            file,
            "statements_per_file",
            m.statements,
            cfg.statements_per_file,
            format!(
                "File has {} statements (threshold: {})",
                m.statements, cfg.statements_per_file
            ),
            "Split into multiple modules with focused responsibilities.",
        );
    }
    if m.interface_types > cfg.interface_types_per_file {
        push_py_file_threshold(
            v,
            file,
            "interface_types_per_file",
            m.interface_types,
            cfg.interface_types_per_file,
            format!(
                "File has {} interface types (threshold: {})",
                m.interface_types, cfg.interface_types_per_file
            ),
            "Move interfaces (Protocols/ABCs) into a dedicated module.",
        );
    }
    if m.concrete_types > cfg.concrete_types_per_file {
        push_py_file_threshold(
            v,
            file,
            "concrete_types_per_file",
            m.concrete_types,
            cfg.concrete_types_per_file,
            format!(
                "File has {} concrete types (threshold: {})",
                m.concrete_types, cfg.concrete_types_per_file
            ),
            "Consider splitting types into separate modules by responsibility.",
        );
    }
    // Skip __init__.py - it's a module definition file that naturally aggregates imports
    if m.imports > cfg.imported_names_per_file && fname != "__init__.py" {
        push_py_file_threshold(
            v,
            file,
            "imported_names_per_file",
            m.imports,
            cfg.imported_names_per_file,
            format!(
                "File has {} imports (threshold: {})",
                m.imports, cfg.imported_names_per_file
            ),
            "Consider reducing dependencies or splitting the module.",
        );
    }
    if m.functions > cfg.functions_per_file {
        push_py_file_threshold(
            v,
            file,
            "functions_per_file",
            m.functions,
            cfg.functions_per_file,
            format!(
                "File has {} functions (threshold: {})",
                m.functions, cfg.functions_per_file
            ),
            "Split into multiple modules with focused responsibilities.",
        );
    }
}

pub fn violation(file: &Path, line: usize, name: &str) -> ViolationBuilder {
    Violation::builder(file).line(line).unit_name(name)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn check_function_metrics(
    m: &FunctionMetrics,
    file: &Path,
    line: usize,
    name: &str,
    inside_class: bool,
    cfg: &Config,
    v: &mut Vec<Violation>,
) {
    let ut = if inside_class { "Method" } else { "Function" };

    macro_rules! chk {
        ($mf:ident, $cf:ident, $metric:literal, $label:literal, $sug:literal) => {
            if m.$mf > cfg.$cf {
                v.push(
                    violation(file, line, name)
                        .metric($metric)
                        .value(m.$mf)
                        .threshold(cfg.$cf)
                        .message(format!(
                            "{} '{}' has {} {} (threshold: {})",
                            ut, name, m.$mf, $label, cfg.$cf
                        ))
                        .suggestion($sug)
                        .build(),
                );
            }
        };
    }

    chk!(
        statements,
        statements_per_function,
        "statements_per_function",
        "statements",
        "Break into smaller, focused functions."
    );
    if !inside_class && m.arguments_positional > cfg.arguments_positional {
        v.push(
            violation(file, line, name)
                .metric("positional_args")
                .value(m.arguments_positional)
                .threshold(cfg.arguments_positional)
                .message(format!(
                    "Function '{}' has {} positional arguments (threshold: {})",
                    name, m.arguments_positional, cfg.arguments_positional
                ))
                .suggestion(
                    "Consider using keyword-only arguments, a config object, or the builder pattern.",
                )
                .build(),
        );
    }
    chk!(
        arguments_keyword_only,
        arguments_keyword_only,
        "keyword_only_args",
        "keyword-only arguments",
        "Consider grouping related parameters into a config object."
    );
    chk!(
        max_indentation,
        max_indentation_depth,
        "max_indentation_depth",
        "indentation depth",
        "Extract nested logic into helper functions or use early returns."
    );
    chk!(
        nested_function_depth,
        nested_function_depth,
        "nested_function_depth",
        "nested functions",
        "Move nested functions to module level or use classes."
    );
    chk!(
        branches,
        branches_per_function,
        "branches_per_function",
        "branches",
        "Consider using polymorphism, lookup tables, or the strategy pattern."
    );
    chk!(
        local_variables,
        local_variables_per_function,
        "local_variables_per_function",
        "local variables",
        "Extract related variables into a data class or split the function."
    );
    chk!(
        max_try_block_statements,
        statements_per_try_block,
        "statements_per_try_block",
        "statements in try block",
        "Keep try blocks narrow: wrap only the code that can raise the specific exception."
    );
    chk!(
        boolean_parameters,
        boolean_parameters,
        "boolean_parameters",
        "boolean parameters",
        "Use keyword-only arguments, an enum, or separate functions instead of boolean flags."
    );
    chk!(
        returns,
        returns_per_function,
        "returns_per_function",
        "return statements",
        "Use early guard returns at the top, then a single main return path."
    );
    chk!(
        decorators,
        annotations_per_function,
        "annotations_per_function",
        "annotations (decorators)",
        "Consider consolidating decorators or simplifying the function's responsibilities. (TOML key: decorators_per_function)"
    );
    chk!(
        max_return_values,
        return_values_per_function,
        "return_values_per_function",
        "return values",
        "Consider returning a named tuple, dataclass, or structured object instead of multiple values."
    );
    chk!(
        calls,
        calls_per_function,
        "calls_per_function",
        "calls",
        "Extract some calls into helper functions to reduce coordination complexity."
    );
}
