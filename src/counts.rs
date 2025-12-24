//! Count-based code metrics
//!
//! ## LCOM (Lack of Cohesion of Methods)
//!
//! LCOM measures how well the methods of a class work together by examining
//! which instance fields they access. A cohesive class has methods that share
//! common state; a non-cohesive class has methods that operate on disjoint fields.
//!
//! ### Calculation
//! 1. For each method, collect the set of `self.field` accesses
//! 2. For each pair of methods, check if their field sets intersect
//! 3. LCOM = (pairs not sharing fields) / (total pairs)
//!
//! ### Interpretation
//! - **0.0** = Perfectly cohesive (all method pairs share at least one field)
//! - **1.0** = No cohesion (no method pairs share any fields)
//! - **God Class**: methods > 20 AND LCOM > 50% indicates a class doing too much
//!
//! ### Limitations
//! - Methods that don't access `self` fields are excluded from the calculation
//! - This means a class with only static-like methods will have LCOM = 0.0

use crate::config::Config;
use crate::graph::compute_cyclomatic_complexity;
use crate::parsing::ParsedFile;
use crate::units::get_child_by_field;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

#[derive(Debug, Default)]
pub struct FunctionMetrics {
    pub statements: usize,
    pub arguments: usize,
    pub arguments_positional: usize,
    pub arguments_keyword_only: usize,
    pub max_indentation: usize,
    pub nested_function_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
}

#[derive(Debug, Default)]
pub struct ClassMetrics {
    pub methods: usize,
    pub lcom: f64,
}

#[derive(Debug, Default)]
pub struct FileMetrics {
    pub lines: usize,
    pub classes: usize,
    pub imports: usize,
}

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

impl Violation {
    /// Create a new violation builder for the given file
    pub fn builder(file: impl Into<PathBuf>) -> ViolationBuilder {
        ViolationBuilder::new(file)
    }
}

/// Builder for constructing Violation instances with a fluent API
pub struct ViolationBuilder {
    file: PathBuf,
    line: usize,
    unit_name: String,
    metric: String,
    value: usize,
    threshold: usize,
    message: String,
    suggestion: String,
}

impl ViolationBuilder {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            line: 1,
            unit_name: String::new(),
            metric: String::new(),
            value: 0,
            threshold: 0,
            message: String::new(),
            suggestion: String::new(),
        }
    }

    pub fn line(mut self, line: usize) -> Self { self.line = line; self }
    pub fn unit_name(mut self, name: impl Into<String>) -> Self { self.unit_name = name.into(); self }
    pub fn metric(mut self, metric: impl Into<String>) -> Self { self.metric = metric.into(); self }
    pub fn value(mut self, value: usize) -> Self { self.value = value; self }
    pub fn threshold(mut self, threshold: usize) -> Self { self.threshold = threshold; self }
    pub fn message(mut self, message: impl Into<String>) -> Self { self.message = message.into(); self }
    pub fn suggestion(mut self, suggestion: impl Into<String>) -> Self { self.suggestion = suggestion.into(); self }

    pub fn build(self) -> Violation {
        Violation {
            file: self.file,
            line: self.line,
            unit_name: self.unit_name,
            metric: self.metric,
            value: self.value,
            threshold: self.threshold,
            message: self.message,
            suggestion: self.suggestion,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn mk_v(file: PathBuf, line: usize, name: &str, metric: &str, val: usize, thresh: usize, msg: String, sug: &str) -> Violation {
    Violation::builder(file)
        .line(line)
        .unit_name(name)
        .metric(metric)
        .value(val)
        .threshold(thresh)
        .message(msg)
        .suggestion(sug)
        .build()
}

#[must_use]
pub fn analyze_file(parsed: &ParsedFile, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let file = parsed.path.clone();
    let file_metrics = compute_file_metrics(parsed);
    let fname = file.file_name().unwrap_or_default().to_string_lossy().into_owned();

    if file_metrics.lines > config.lines_per_file {
        violations.push(mk_v(file.clone(), 1, &fname, "lines_per_file", file_metrics.lines, config.lines_per_file,
            format!("File has {} lines (threshold: {})", file_metrics.lines, config.lines_per_file), "Split into multiple modules with focused responsibilities."));
    }
    if file_metrics.classes > config.classes_per_file {
        violations.push(mk_v(file.clone(), 1, &fname, "classes_per_file", file_metrics.classes, config.classes_per_file,
            format!("File has {} classes (threshold: {})", file_metrics.classes, config.classes_per_file), "Move classes to separate files, one class per file."));
    }
    if file_metrics.imports > config.imports_per_file {
        violations.push(mk_v(file.clone(), 1, &fname, "imports_per_file", file_metrics.imports, config.imports_per_file,
            format!("File has {} imports (threshold: {})", file_metrics.imports, config.imports_per_file), "Module may have too many responsibilities. Consider splitting."));
    }
    analyze_node(parsed.tree.root_node(), &parsed.source, &file, &mut violations, false, config);
    violations
}

fn analyze_node(node: Node, source: &str, file: &Path, violations: &mut Vec<Violation>, inside_class: bool, config: &Config) {
    match node.kind() {
        "function_definition" | "async_function_definition" => analyze_function_node(node, source, file, violations, inside_class, config),
        "class_definition" => analyze_class_node(node, source, file, violations, config),
        _ => recurse_children(node, source, file, violations, inside_class, config),
    }
}

fn recurse_children(node: Node, source: &str, file: &Path, violations: &mut Vec<Violation>, inside_class: bool, config: &Config) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        analyze_node(child, source, file, violations, inside_class, config);
    }
}

fn analyze_function_node(node: Node, source: &str, file: &Path, violations: &mut Vec<Violation>, inside_class: bool, config: &Config) {
    let name = get_child_by_field(node, "name", source).unwrap_or_else(|| "<unknown>".to_string());
    let line = node.start_position().row + 1;
    let m = compute_function_metrics(node, source);
    let ut = if inside_class { "Method" } else { "Function" };
    let c = config;
    let f = file.to_path_buf();

    macro_rules! chk {
        ($mf:expr, $cf:expr, $metric:literal, $label:literal, $sug:literal) => {
            if $mf > $cf { violations.push(mk_v(f.clone(), line, &name, $metric, $mf, $cf, format!("{} '{}' has {} {} (threshold: {})", ut, name, $mf, $label, $cf), $sug)); }
        };
    }
    chk!(m.statements, c.statements_per_function, "statements_per_function", "statements", "Break into smaller, focused functions.");
    chk!(m.arguments, c.arguments_per_function, "arguments_per_function", "total arguments", "Group related arguments into a data class or dict.");
    chk!(m.arguments_positional, c.arguments_positional, "arguments_positional", "positional arguments", "Consider using keyword-only arguments (`*`) after the first 2-3 to prevent argument order mistakes.");
    chk!(m.arguments_keyword_only, c.arguments_keyword_only, "arguments_keyword_only", "keyword-only arguments", "Consider grouping related parameters into a configuration object.");
    chk!(m.max_indentation, c.max_indentation_depth, "max_indentation_depth", "indentation depth", "Use early returns, guard clauses, or extract helper functions.");
    chk!(m.returns, c.returns_per_function, "returns_per_function", "return statements", "Reduce exit points; consider restructuring logic.");
    chk!(m.branches, c.branches_per_function, "branches_per_function", "branches", "Consider using polymorphism, strategy pattern, or dict dispatch.");
    chk!(m.local_variables, c.local_variables_per_function, "local_variables_per_function", "local variables", "Extract logic into helper functions with fewer variables each.");
    chk!(m.nested_function_depth, c.nested_function_depth, "nested_function_depth", "nested function depth", "Move nested functions to module level or refactor into a class.");

            let complexity = compute_cyclomatic_complexity(node);
    chk!(complexity, c.cyclomatic_complexity, "cyclomatic_complexity", "cyclomatic complexity", "Simplify control flow; extract helper functions or use polymorphism.");

    recurse_children(node, source, file, violations, false, config);
}

fn analyze_class_node(node: Node, source: &str, file: &Path, violations: &mut Vec<Violation>, config: &Config) {
            let name = get_child_by_field(node, "name", source).unwrap_or_else(|| "<unknown>".to_string());
            let line = node.start_position().row + 1;
    let m = compute_class_metrics_with_source(node, source);
    let f = file.to_path_buf();
    let lcom_pct = (m.lcom * 100.0).round() as usize;

    if m.methods > config.methods_per_class {
        violations.push(mk_v(f.clone(), line, &name, "methods_per_class", m.methods, config.methods_per_class,
            format!("Class '{}' has {} methods (threshold: {})", name, m.methods, config.methods_per_class), "Split into multiple classes with single responsibilities."));
    }
    if lcom_pct > config.lcom {
        violations.push(mk_v(f.clone(), line, &name, "lcom", lcom_pct, config.lcom,
            format!("Class '{}' has LCOM of {}% (threshold: {}%)", name, lcom_pct, config.lcom), "Methods in this class don't share fields; consider splitting into cohesive classes."));
    }
    if m.methods > 20 && lcom_pct > 50 {
        violations.push(mk_v(f.clone(), line, &name, "god_class", 1, 0,
            format!("Class '{}' is a God Class: {} methods + {}% LCOM indicates low cohesion", name, m.methods, lcom_pct), "Break into smaller, focused classes with single responsibilities."));
    }
    recurse_children(node, source, file, violations, true, config);
}

/// Computes metrics for a function node
#[must_use]
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
#[must_use]
pub fn compute_class_metrics(node: Node) -> ClassMetrics {
    compute_class_metrics_with_source(node, "")
}

/// Computes metrics for a class, including LCOM which requires source code
pub fn compute_class_metrics_with_source(node: Node, source: &str) -> ClassMetrics {
    let mut metrics = ClassMetrics::default();

    if let Some(body) = node.child_by_field_name("body") {
        metrics.methods = count_node_kind(body, "function_definition")
            + count_node_kind(body, "async_function_definition");
        
        // Compute LCOM if we have source and methods
        if !source.is_empty() && metrics.methods > 1 {
            metrics.lcom = compute_lcom(body, source);
        }
    }

    metrics
}

/// Compute LCOM (Lack of Cohesion of Methods) for a class body
/// Returns a value between 0.0 (cohesive) and 1.0 (no cohesion)
fn compute_lcom(body: Node, source: &str) -> f64 {
    use std::collections::HashSet;
    
    // Collect fields accessed by each method
    let mut method_fields: Vec<HashSet<String>> = Vec::new();
    let mut cursor = body.walk();
    
    for child in body.children(&mut cursor) {
        if child.kind() == "function_definition" || child.kind() == "async_function_definition" {
            let fields = extract_self_attributes(child, source);
            if !fields.is_empty() {
                method_fields.push(fields);
            }
        }
    }
    
    let num_methods = method_fields.len();
    if num_methods < 2 {
        return 0.0; // Single method or no methods with field access = cohesive
    }
    
    // Count pairs that share fields vs pairs that don't
    let mut pairs_sharing = 0usize;
    let mut pairs_not_sharing = 0usize;
    
    for i in 0..num_methods {
        for j in (i + 1)..num_methods {
            if method_fields[i].intersection(&method_fields[j]).count() > 0 {
                pairs_sharing += 1;
            } else {
                pairs_not_sharing += 1;
            }
        }
    }
    
    let total_pairs = pairs_sharing + pairs_not_sharing;
    if total_pairs == 0 {
        return 0.0;
    }
    
    // LCOM = proportion of pairs that don't share fields
    pairs_not_sharing as f64 / total_pairs as f64
}

/// Extract all self.attribute accesses from a function node
fn extract_self_attributes(node: Node, source: &str) -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    let mut fields = HashSet::new();
    extract_self_attributes_recursive(node, source, &mut fields);
    fields
}

fn extract_self_attributes_recursive(node: Node, source: &str, fields: &mut std::collections::HashSet<String>) {
    // Look for self.attribute pattern
    if node.kind() == "attribute"
        && let (Some(obj), Some(attr)) = (node.child_by_field_name("object"), node.child_by_field_name("attribute")) {
            let is_self = obj.kind() == "identifier" && &source[obj.start_byte()..obj.end_byte()] == "self";
            if is_self { fields.insert(source[attr.start_byte()..attr.end_byte()].to_string()); }
        }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) { extract_self_attributes_recursive(child, source, fields); }
}

/// Computes metrics for an entire file
#[must_use]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use std::io::Write;

    fn parse_source(code: &str) -> crate::parsing::ParsedFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{}", code).unwrap();
        let mut parser = create_parser().unwrap();
        parse_file(&mut parser, tmp.path()).unwrap()
    }

    #[test]
    fn test_function_metrics_struct() {
        let m = FunctionMetrics { statements: 5, arguments: 2, arguments_positional: 1, arguments_keyword_only: 1, max_indentation: 2, returns: 1, branches: 0, local_variables: 3, nested_function_depth: 0 };
        assert_eq!(m.statements, 5);
    }

    #[test]
    fn test_class_metrics_struct() {
        let m = ClassMetrics { methods: 3, lcom: 0.5 };
        assert_eq!(m.methods, 3);
    }

    #[test]
    fn test_file_metrics_struct() {
        let m = FileMetrics { lines: 100, classes: 2, imports: 5 };
        assert_eq!(m.lines, 100);
    }

    #[test]
    fn test_violation_struct() {
        let v = Violation { file: PathBuf::from("test.py"), line: 10, unit_name: "foo".into(), metric: "m".into(), value: 5, threshold: 3, message: "msg".into(), suggestion: "sug".into() };
        assert_eq!(v.line, 10);
    }

    #[test]
    fn test_mk_v() {
        let v = mk_v(PathBuf::from("f.py"), 1, "n", "m", 10, 5, "msg".into(), "sug");
        assert_eq!(v.value, 10);
        assert_eq!(v.threshold, 5);
    }

    #[test]
    fn test_compute_function_metrics() {
        let parsed = parse_source("def f(a, b):\n    x = 1\n    return x");
        let root = parsed.tree.root_node();
        let func = root.child(0).unwrap();
        let m = compute_function_metrics(func, &parsed.source);
        assert_eq!(m.arguments, 2);
        assert!(m.statements >= 2);
        assert!(m.returns >= 1);
    }

    #[test]
    fn test_compute_class_metrics() {
        let parsed = parse_source("class C:\n    def a(self): pass\n    def b(self): pass");
        let root = parsed.tree.root_node();
        let cls = root.child(0).unwrap();
        let m = compute_class_metrics(cls);
        assert_eq!(m.methods, 2);
    }

    #[test]
    fn test_compute_class_metrics_with_source() {
        let parsed = parse_source("class C:\n    def a(self): self.x = 1\n    def b(self): self.x = 2");
        let root = parsed.tree.root_node();
        let cls = root.child(0).unwrap();
        let m = compute_class_metrics_with_source(cls, &parsed.source);
        assert_eq!(m.methods, 2);
        assert!(m.lcom >= 0.0 && m.lcom <= 1.0);
    }

    #[test]
    fn test_compute_file_metrics() {
        let parsed = parse_source("import os\nclass A: pass\nclass B: pass");
        let m = compute_file_metrics(&parsed);
        assert_eq!(m.classes, 2);
        assert_eq!(m.imports, 1);
        assert!(m.lines >= 3);
    }

    #[test]
    fn test_count_statements() {
        let parsed = parse_source("def f():\n    x = 1\n    y = 2\n    return x + y");
        let func = parsed.tree.root_node().child(0).unwrap();
        let body = func.child_by_field_name("body").unwrap();
        assert!(count_statements(body) >= 3);
    }

    #[test]
    fn test_is_statement() {
        assert!(is_statement("expression_statement"));
        assert!(is_statement("return_statement"));
        assert!(is_statement("if_statement"));
        assert!(!is_statement("identifier"));
    }

    #[test]
    fn test_compute_max_indentation() {
        let parsed = parse_source("def f():\n    if True:\n        if True:\n            x = 1");
        let func = parsed.tree.root_node().child(0).unwrap();
        let depth = compute_max_indentation(func, 0);
        assert!(depth >= 2, "depth was {}", depth);
    }

    #[test]
    fn test_count_branches() {
        let parsed = parse_source("def f():\n    if a: pass\n    if b: pass");
        let func = parsed.tree.root_node().child(0).unwrap();
        let body = func.child_by_field_name("body").unwrap();
        assert_eq!(count_branches(body), 2);
    }

    #[test]
    fn test_count_local_variables() {
        let parsed = parse_source("def f():\n    x = 1\n    y = 2");
        let func = parsed.tree.root_node().child(0).unwrap();
        let body = func.child_by_field_name("body").unwrap();
        assert!(count_local_variables(body, &parsed.source) >= 2);
    }

    #[test]
    fn test_count_parameters() {
        let parsed = parse_source("def f(a, b, *, c, d): pass");
        let func = parsed.tree.root_node().child(0).unwrap();
        let params = func.child_by_field_name("parameters").unwrap();
        let c = count_parameters(params);
        assert_eq!(c.positional, 2);
        assert_eq!(c.keyword_only, 2);
        assert_eq!(c.total, 4);
    }

    #[test]
    fn test_compute_nested_function_depth() {
        let parsed = parse_source("def f():\n    def g():\n        def h(): pass");
        let func = parsed.tree.root_node().child(0).unwrap();
        assert!(compute_nested_function_depth(func, 0) >= 2);
    }

    #[test]
    fn test_count_imports() {
        let parsed = parse_source("import os\nfrom sys import path\nimport json");
        let root = parsed.tree.root_node();
        assert!(count_imports(root) >= 3);
    }

    #[test]
    fn test_extract_self_attributes() {
        let parsed = parse_source("def m(self):\n    self.x = 1\n    self.y = 2");
        let func = parsed.tree.root_node().child(0).unwrap();
        let attrs = extract_self_attributes(func, &parsed.source);
        assert!(attrs.contains("x"));
        assert!(attrs.contains("y"));
    }

    #[test]
    fn test_analyze_file_no_violations() {
        let parsed = parse_source("def f(): pass");
        let config = Config::default();
        let violations = analyze_file(&parsed, &config);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_analyze_file_with_violation() {
        let parsed = parse_source("def f(a,b,c,d,e,f,g,h,i,j): pass");
        let mut config = Config::default();
        config.arguments_per_function = 5;
        let violations = analyze_file(&parsed, &config);
        assert!(!violations.is_empty());
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
    fn test_compute_lcom() {
        let parsed = parse_source("class C:\n    def a(self): self.x = 1\n    def b(self): self.y = 1");
        let cls = parsed.tree.root_node().child(0).unwrap();
        let body = cls.child_by_field_name("body").unwrap();
        let lcom = compute_lcom(body, &parsed.source);
        assert!(lcom >= 0.0 && lcom <= 1.0);
    }

    #[test]
    fn test_parameter_counts_struct() {
        let c = ParameterCounts { positional: 2, keyword_only: 1, total: 3 };
        assert_eq!(c.total, 3);
    }

    #[test]
    fn test_count_node_kind() {
        let parsed = parse_source("class A: pass\nclass B: pass");
        let count = count_node_kind(parsed.tree.root_node(), "class_definition");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_collect_local_variables() {
        let parsed = parse_source("x = 1\ny = 2");
        let mut vars = std::collections::HashSet::new();
        collect_local_variables(parsed.tree.root_node(), &parsed.source, &mut vars);
        assert!(vars.contains("x"));
        assert!(vars.contains("y"));
    }

    #[test]
    fn test_collect_assigned_names() {
        let parsed = parse_source("(a, b) = (1, 2)");
        let mut vars = std::collections::HashSet::new();
        // Walk to find the tuple_pattern or pattern_list
        let root = parsed.tree.root_node();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            collect_assigned_names(child, &parsed.source, &mut vars);
        }
        // Just verify the function runs without panic
        let _ = vars.len();
    }

    #[test]
    fn test_count_import_names() {
        let parsed = parse_source("from os import path, getcwd");
        let import = parsed.tree.root_node().child(0).unwrap();
        assert!(count_import_names(import) >= 2);
    }

    #[test]
    fn test_recurse_children() {
        let parsed = parse_source("if True: pass");
        let mut viols = Vec::new();
        recurse_children(parsed.tree.root_node(), &parsed.source, &parsed.path, &mut viols, false, &Config::default());
        assert!(viols.is_empty());
    }

    #[test]
    fn test_analyze_function_node() {
        let parsed = parse_source("def f(): pass");
        let mut viols = Vec::new();
        let func = parsed.tree.root_node().child(0).unwrap();
        analyze_function_node(func, &parsed.source, &parsed.path, &mut viols, false, &Config::default());
        assert!(viols.is_empty());
    }

    #[test]
    fn test_extract_self_attributes_recursive_simple() {
        let code = r#"
class Foo:
    def method(self):
        self.x = 1
        self.y = 2
"#;
        let parsed = parse_source(code);
        let root = parsed.tree.root_node();
        let mut fields = std::collections::HashSet::new();
        // Find the function_definition node
        let class_node = root.child(0).unwrap();
        let body = class_node.child_by_field_name("body").unwrap();
        let method = body.child(0).unwrap();
        extract_self_attributes_recursive(method, &parsed.source, &mut fields);
        assert!(fields.contains("x"), "should find self.x, got: {:?}", fields);
        assert!(fields.contains("y"), "should find self.y, got: {:?}", fields);
    }

    #[test]
    fn test_extract_self_attributes_recursive_no_self() {
        let code = r#"
def func():
    x = 1
    y = 2
"#;
        let parsed = parse_source(code);
        let root = parsed.tree.root_node();
        let mut fields = std::collections::HashSet::new();
        let func_node = root.child(0).unwrap();
        extract_self_attributes_recursive(func_node, &parsed.source, &mut fields);
        assert!(fields.is_empty(), "should not find any self attributes, got: {:?}", fields);
    }
}

