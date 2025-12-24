//! Count-based code metrics

use crate::config::Config;
use crate::graph::compute_cyclomatic_complexity;
use crate::parsing::ParsedFile;
use crate::units::get_child_by_field;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

/// Metrics computed for a function or method
#[derive(Debug, Default)]
pub struct FunctionMetrics {
    pub statements: usize,
    pub arguments: usize,             // Total count for backwards compatibility
    pub arguments_positional: usize,   // Positional-only + positional-or-keyword
    pub arguments_keyword_only: usize, // Keyword-only (after *args or *)
    pub max_indentation: usize,
    pub nested_function_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
}

/// Metrics computed for a class
#[derive(Debug, Default)]
pub struct ClassMetrics {
    pub methods: usize,
}

/// Metrics computed for a file/module
#[derive(Debug, Default)]
pub struct FileMetrics {
    pub lines: usize,
    pub classes: usize,
    pub imports: usize,
}

/// A violation of a metric threshold
#[derive(Debug)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub unit_name: String,
    pub metric: String,
    pub value: usize,
    pub threshold: usize,
    pub message: String,
    pub suggestion: String,
}

/// Analyzes a parsed file and returns all violations
pub fn analyze_file(parsed: &ParsedFile, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let file = parsed.path.clone();

    // File-level metrics
    let file_metrics = compute_file_metrics(parsed);

    if file_metrics.lines > config.lines_per_file {
        violations.push(Violation {
            file: file.clone(),
            line: 1,
            unit_name: file.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            metric: "lines_per_file".to_string(),
            value: file_metrics.lines,
            threshold: config.lines_per_file,
            message: format!("File has {} lines (threshold: {})", file_metrics.lines, config.lines_per_file),
            suggestion: "Split into multiple modules with focused responsibilities.".to_string(),
        });
    }

    if file_metrics.classes > config.classes_per_file {
        violations.push(Violation {
            file: file.clone(),
            line: 1,
            unit_name: file.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            metric: "classes_per_file".to_string(),
            value: file_metrics.classes,
            threshold: config.classes_per_file,
            message: format!("File has {} classes (threshold: {})", file_metrics.classes, config.classes_per_file),
            suggestion: "Move classes to separate files, one class per file.".to_string(),
        });
    }

    if file_metrics.imports > config.imports_per_file {
        violations.push(Violation {
            file: file.clone(),
            line: 1,
            unit_name: file.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            metric: "imports_per_file".to_string(),
            value: file_metrics.imports,
            threshold: config.imports_per_file,
            message: format!("File has {} imports (threshold: {})", file_metrics.imports, config.imports_per_file),
            suggestion: "Module may have too many responsibilities. Consider splitting.".to_string(),
        });
    }

    // Walk AST for class and function metrics
    analyze_node(parsed.tree.root_node(), &parsed.source, &file, &mut violations, false, config);

    violations
}

fn analyze_node(node: Node, source: &str, file: &Path, violations: &mut Vec<Violation>, inside_class: bool, config: &Config) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = get_child_by_field(node, "name", source).unwrap_or_else(|| "<unknown>".to_string());
            let line = node.start_position().row + 1;
            let metrics = compute_function_metrics(node, source);
            let unit_type = if inside_class { "Method" } else { "Function" };

            if metrics.statements > config.statements_per_function {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "statements_per_function".to_string(),
                    value: metrics.statements,
                    threshold: config.statements_per_function,
                    message: format!("{} '{}' has {} statements (threshold: {})", unit_type, name, metrics.statements, config.statements_per_function),
                    suggestion: "Break into smaller, focused functions.".to_string(),
                });
            }

            if metrics.arguments > config.arguments_per_function {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "arguments_per_function".to_string(),
                    value: metrics.arguments,
                    threshold: config.arguments_per_function,
                    message: format!("{} '{}' has {} total arguments (threshold: {})", unit_type, name, metrics.arguments, config.arguments_per_function),
                    suggestion: "Group related arguments into a data class or dict.".to_string(),
                });
            }

            if metrics.arguments_positional > config.arguments_positional {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "arguments_positional".to_string(),
                    value: metrics.arguments_positional,
                    threshold: config.arguments_positional,
                    message: format!("{} '{}' has {} positional arguments (threshold: {})", unit_type, name, metrics.arguments_positional, config.arguments_positional),
                    suggestion: "Consider using keyword-only arguments (`*`) after the first 2-3 to prevent argument order mistakes.".to_string(),
                });
            }

            if metrics.arguments_keyword_only > config.arguments_keyword_only {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "arguments_keyword_only".to_string(),
                    value: metrics.arguments_keyword_only,
                    threshold: config.arguments_keyword_only,
                    message: format!("{} '{}' has {} keyword-only arguments (threshold: {})", unit_type, name, metrics.arguments_keyword_only, config.arguments_keyword_only),
                    suggestion: "Consider grouping related parameters into a configuration object.".to_string(),
                });
            }

            if metrics.max_indentation > config.max_indentation_depth {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "max_indentation_depth".to_string(),
                    value: metrics.max_indentation,
                    threshold: config.max_indentation_depth,
                    message: format!("{} '{}' has indentation depth {} (threshold: {})", unit_type, name, metrics.max_indentation, config.max_indentation_depth),
                    suggestion: "Use early returns, guard clauses, or extract helper functions.".to_string(),
                });
            }

            if metrics.returns > config.returns_per_function {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "returns_per_function".to_string(),
                    value: metrics.returns,
                    threshold: config.returns_per_function,
                    message: format!("{} '{}' has {} return statements (threshold: {})", unit_type, name, metrics.returns, config.returns_per_function),
                    suggestion: "Reduce exit points; consider restructuring logic.".to_string(),
                });
            }

            if metrics.branches > config.branches_per_function {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "branches_per_function".to_string(),
                    value: metrics.branches,
                    threshold: config.branches_per_function,
                    message: format!("{} '{}' has {} branches (threshold: {})", unit_type, name, metrics.branches, config.branches_per_function),
                    suggestion: "Consider using polymorphism, strategy pattern, or dict dispatch.".to_string(),
                });
            }

            if metrics.local_variables > config.local_variables_per_function {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "local_variables_per_function".to_string(),
                    value: metrics.local_variables,
                    threshold: config.local_variables_per_function,
                    message: format!("{} '{}' has {} local variables (threshold: {})", unit_type, name, metrics.local_variables, config.local_variables_per_function),
                    suggestion: "Extract logic into helper functions with fewer variables each.".to_string(),
                });
            }

            if metrics.nested_function_depth > config.nested_function_depth {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "nested_function_depth".to_string(),
                    value: metrics.nested_function_depth,
                    threshold: config.nested_function_depth,
                    message: format!("{} '{}' has nested function depth {} (threshold: {})", unit_type, name, metrics.nested_function_depth, config.nested_function_depth),
                    suggestion: "Move nested functions to module level or refactor into a class.".to_string(),
                });
            }

            // Cyclomatic complexity
            let complexity = compute_cyclomatic_complexity(node);
            if complexity > config.cyclomatic_complexity {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "cyclomatic_complexity".to_string(),
                    value: complexity,
                    threshold: config.cyclomatic_complexity,
                    message: format!("{} '{}' has cyclomatic complexity {} (threshold: {})", unit_type, name, complexity, config.cyclomatic_complexity),
                    suggestion: "Simplify control flow; extract helper functions or use polymorphism.".to_string(),
                });
            }

            // Recurse into function body for nested functions
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    analyze_node(child, source, file, violations, false, config);
                }
            }
        }
        "class_definition" => {
            let name = get_child_by_field(node, "name", source).unwrap_or_else(|| "<unknown>".to_string());
            let line = node.start_position().row + 1;
            let metrics = compute_class_metrics(node);

            if metrics.methods > config.methods_per_class {
                violations.push(Violation {
                    file: file.to_path_buf(),
                    line,
                    unit_name: name.clone(),
                    metric: "methods_per_class".to_string(),
                    value: metrics.methods,
                    threshold: config.methods_per_class,
                    message: format!("Class '{}' has {} methods (threshold: {})", name, metrics.methods, config.methods_per_class),
                    suggestion: "Split into multiple classes with single responsibilities.".to_string(),
                });
            }

            // Recurse into class body
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    analyze_node(child, source, file, violations, true, config);
                }
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    analyze_node(child, source, file, violations, inside_class, config);
                }
            }
        }
    }
}

/// Computes metrics for a function node
pub fn compute_function_metrics(node: Node, source: &str) -> FunctionMetrics {
    let mut metrics = FunctionMetrics::default();

    // Count arguments
    if let Some(params) = node.child_by_field_name("parameters") {
        let counts = count_parameters(params);
        metrics.arguments = counts.total;
        metrics.arguments_positional = counts.positional;
        metrics.arguments_keyword_only = counts.keyword_only;
    }

    // Analyze function body
    if let Some(body) = node.child_by_field_name("body") {
        metrics.statements = count_statements(body);
        metrics.max_indentation = compute_max_indentation(body, 0);
        metrics.returns = count_node_kind(body, "return_statement");
        metrics.branches = count_branches(body);
        metrics.local_variables = count_local_variables(body, source);
        metrics.nested_function_depth = compute_nested_function_depth(body, 0);
    }

    metrics
}

/// Computes metrics for a class node
pub fn compute_class_metrics(node: Node) -> ClassMetrics {
    let mut metrics = ClassMetrics::default();

    if let Some(body) = node.child_by_field_name("body") {
        metrics.methods = count_node_kind(body, "function_definition")
            + count_node_kind(body, "async_function_definition");
    }

    metrics
}

/// Computes metrics for an entire file
pub fn compute_file_metrics(parsed: &ParsedFile) -> FileMetrics {
    let root = parsed.tree.root_node();
    FileMetrics {
        lines: parsed.source.lines().count(),
        classes: count_node_kind(root, "class_definition"),
        imports: count_imports(root),
    }
}

/// Breakdown of parameter counts
struct ParameterCounts {
    positional: usize,
    keyword_only: usize,
    total: usize,
}

fn count_parameters(params: Node) -> ParameterCounts {
    let mut positional = 0;
    let mut keyword_only = 0;
    let mut after_star = false;

    for i in 0..params.child_count() {
        if let Some(child) = params.child(i) {
            match child.kind() {
                "list_splat_pattern" => {
                    // *args - marks start of keyword-only section
                    // *args itself is not counted as a regular parameter
                    after_star = true;
                }
                "keyword_separator" => {
                    // bare * - marks start of keyword-only section without *args
                    after_star = true;
                }
                "dictionary_splat_pattern" => {
                    // **kwargs - not counted as a regular parameter
                }
                "identifier" | "typed_parameter" | "default_parameter"
                | "typed_default_parameter" => {
                    if after_star {
                        keyword_only += 1;
                    } else {
                        positional += 1;
                    }
                }
                _ => {}
            }
        }
    }

    ParameterCounts {
        positional,
        keyword_only,
        total: positional + keyword_only,
    }
}

fn count_statements(node: Node) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if is_statement(child.kind()) {
            count += 1;
        }
        // Recurse into compound statements
        count += count_statements(child);
    }

    count
}

fn is_statement(kind: &str) -> bool {
    matches!(
        kind,
        "expression_statement"
            | "return_statement"
            | "pass_statement"
            | "break_statement"
            | "continue_statement"
            | "raise_statement"
            | "assert_statement"
            | "global_statement"
            | "nonlocal_statement"
            | "import_statement"
            | "import_from_statement"
            | "if_statement"
            | "for_statement"
            | "while_statement"
            | "try_statement"
            | "with_statement"
            | "match_statement"
            | "function_definition"
            | "async_function_definition"
            | "class_definition"
            | "decorated_definition"
    )
}

fn compute_max_indentation(node: Node, current_depth: usize) -> usize {
    let mut max_depth = current_depth;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let child_depth = match child.kind() {
            "if_statement" | "for_statement" | "while_statement" | "try_statement"
            | "with_statement" | "match_statement" => {
                compute_max_indentation(child, current_depth + 1)
            }
            "block" => compute_max_indentation(child, current_depth),
            _ => compute_max_indentation(child, current_depth),
        };
        max_depth = max_depth.max(child_depth);
    }

    max_depth
}

pub(crate) fn count_node_kind(node: Node, kind: &str) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            count += 1;
        }
        count += count_node_kind(child, kind);
    }

    count
}

fn count_branches(node: Node) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "if_statement" | "elif_clause" | "else_clause" => count += 1,
            _ => {}
        }
        count += count_branches(child);
    }

    count
}

fn count_local_variables(node: Node, source: &str) -> usize {
    use std::collections::HashSet;
    let mut variables = HashSet::new();
    collect_local_variables(node, source, &mut variables);
    variables.len()
}

fn collect_local_variables(node: Node, source: &str, variables: &mut std::collections::HashSet<String>) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "assignment" => {
                // Left side of assignment contains variable names
                if let Some(left) = child.child_by_field_name("left") {
                    collect_assigned_names(left, source, variables);
                }
            }
            "augmented_assignment" => {
                if let Some(left) = child.child_by_field_name("left") {
                    collect_assigned_names(left, source, variables);
                }
            }
            "for_statement" => {
                if let Some(left) = child.child_by_field_name("left") {
                    collect_assigned_names(left, source, variables);
                }
            }
            // Don't recurse into nested functions
            "function_definition" | "async_function_definition" => continue,
            _ => {}
        }
        collect_local_variables(child, source, variables);
    }
}

fn collect_assigned_names(node: Node, source: &str, variables: &mut std::collections::HashSet<String>) {
    match node.kind() {
        "identifier" => {
            let name = source[node.start_byte()..node.end_byte()].to_string();
            variables.insert(name);
        }
        "tuple_pattern" | "list_pattern" | "pattern_list" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_assigned_names(child, source, variables);
            }
        }
        _ => {}
    }
}

fn compute_nested_function_depth(node: Node, current_depth: usize) -> usize {
    let mut max_depth = current_depth;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        let child_depth = if matches!(child.kind(), "function_definition" | "async_function_definition") {
            compute_nested_function_depth(child, current_depth + 1)
        } else {
            compute_nested_function_depth(child, current_depth)
        };
        max_depth = max_depth.max(child_depth);
    }

    max_depth
}

fn count_imports(node: Node) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" => {
                // Count each imported name
                count += count_import_names(child);
            }
            "import_from_statement" => {
                // Count each imported name
                count += count_import_names(child);
            }
            _ => {}
        }
    }

    count
}

fn count_import_names(node: Node) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" | "aliased_import" => count += 1,
            "wildcard_import" => count += 1,
            _ => {}
        }
    }

    // If no children matched, it's a simple `import foo` with just one name
    if count == 0 { 1 } else { count }
}

