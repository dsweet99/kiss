//! Count-based code metrics for Rust

use crate::config::Config;
use crate::counts::Violation;
use crate::rust_parsing::ParsedRustFile;
use std::path::PathBuf;
use syn::visit::Visit;
use syn::{Block, Expr, ImplItem, Item, Pat, Stmt};

/// Metrics computed for a Rust function or method
#[derive(Debug, Default)]
pub struct RustFunctionMetrics {
    pub statements: usize,
    pub arguments: usize,
    pub max_indentation: usize,
    pub nested_function_depth: usize, // closures
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub cyclomatic_complexity: usize,
}

/// Metrics computed for a Rust struct/enum (type)
#[derive(Debug, Default)]
pub struct RustTypeMetrics {
    pub methods: usize,
}

/// Metrics computed for a Rust file
#[derive(Debug, Default)]
pub struct RustFileMetrics {
    pub lines: usize,
    pub types: usize, // structs + enums
    pub imports: usize, // use statements
}

/// Analyzes a parsed Rust file and returns all violations
pub fn analyze_rust_file(parsed: &ParsedRustFile, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let file = parsed.path.clone();

    // File-level metrics
    let file_metrics = compute_rust_file_metrics(parsed);

    if file_metrics.lines > config.lines_per_file {
        violations.push(Violation {
            file: file.clone(),
            line: 1,
            unit_name: file.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            metric: String::from("lines_per_file"),
            value: file_metrics.lines,
            threshold: config.lines_per_file,
            message: format!("File has {} lines (threshold: {})", file_metrics.lines, config.lines_per_file),
            suggestion: String::from("Split into multiple modules with focused responsibilities."),
        });
    }

    if file_metrics.types > config.classes_per_file {
        violations.push(Violation {
            file: file.clone(),
            line: 1,
            unit_name: file.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            metric: String::from("types_per_file"),
            value: file_metrics.types,
            threshold: config.classes_per_file,
            message: format!("File has {} types (threshold: {})", file_metrics.types, config.classes_per_file),
            suggestion: String::from("Move types to separate files."),
        });
    }

    if file_metrics.imports > config.imports_per_file {
        violations.push(Violation {
            file: file.clone(),
            line: 1,
            unit_name: file.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            metric: String::from("imports_per_file"),
            value: file_metrics.imports,
            threshold: config.imports_per_file,
            message: format!("File has {} use statements (threshold: {})", file_metrics.imports, config.imports_per_file),
            suggestion: String::from("Module may have too many responsibilities. Consider splitting."),
        });
    }

    // Analyze functions and impl blocks
    let mut analyzer = RustAnalyzer::new(&file, config, &mut violations);
    for item in &parsed.ast.items {
        analyzer.analyze_item(item);
    }

    violations
}

struct RustAnalyzer<'a> {
    file: &'a PathBuf,
    config: &'a Config,
    violations: &'a mut Vec<Violation>,
}

impl<'a> RustAnalyzer<'a> {
    fn new(file: &'a PathBuf, config: &'a Config, violations: &'a mut Vec<Violation>) -> Self {
        Self { file, config, violations }
    }

    fn analyze_item(&mut self, item: &Item) {
        match item {
            Item::Fn(func) => {
                let name = func.sig.ident.to_string();
                let line = func.sig.ident.span().start().line;
                self.analyze_function(&name, line, &func.sig.inputs, &func.block, "Function");
            }
            Item::Impl(impl_block) => {
                // Count methods for type metrics
                let method_count = impl_block
                    .items
                    .iter()
                    .filter(|item| matches!(item, ImplItem::Fn(_)))
                    .count();

                let type_name = get_impl_type_name(impl_block);

                if method_count > self.config.methods_per_class {
                    let line = impl_block.impl_token.span.start().line;
                    self.violations.push(Violation {
                        file: self.file.clone(),
                        line,
                        unit_name: type_name.clone().unwrap_or_else(|| String::from("<impl>")),
                        metric: String::from("methods_per_type"),
                        value: method_count,
                        threshold: self.config.methods_per_class,
                        message: format!(
                            "Type '{}' has {} methods (threshold: {})",
                            type_name.as_deref().unwrap_or("<impl>"),
                            method_count,
                            self.config.methods_per_class
                        ),
                        suggestion: String::from("Split into multiple impl blocks or extract functionality."),
                    });
                }

                // Check LCOM for impl blocks with multiple methods
                let lcom_pct = if method_count > 1 {
                    let lcom = compute_rust_lcom(impl_block);
                    let pct = (lcom * 100.0).round() as usize;
                    if pct > self.config.lcom {
                        let line = impl_block.impl_token.span.start().line;
                        self.violations.push(Violation {
                            file: self.file.clone(),
                            line,
                            unit_name: type_name.clone().unwrap_or_else(|| String::from("<impl>")),
                            metric: String::from("lcom"),
                            value: pct,
                            threshold: self.config.lcom,
                            message: format!(
                                "Type '{}' has LCOM of {}% (threshold: {}%)",
                                type_name.as_deref().unwrap_or("<impl>"),
                                pct,
                                self.config.lcom
                            ),
                            suggestion: String::from("Methods in this impl don't share fields; consider splitting."),
                        });
                    }
                    pct
                } else {
                    0
                };

                // God Class indicator: methods > 20 AND LCOM > 50%
                if method_count > 20 && lcom_pct > 50 {
                    let line = impl_block.impl_token.span.start().line;
                    self.violations.push(Violation {
                        file: self.file.clone(),
                        line,
                        unit_name: type_name.clone().unwrap_or_else(|| String::from("<impl>")),
                        metric: String::from("god_class"),
                        value: 1,
                        threshold: 0,
                        message: format!(
                            "Type '{}' is a God Class: {} methods + {}% LCOM indicates low cohesion",
                            type_name.as_deref().unwrap_or("<impl>"),
                            method_count,
                            lcom_pct
                        ),
                        suggestion: String::from("Break into smaller, focused impl blocks with single responsibilities."),
                    });
                }

                // Analyze each method
                for impl_item in &impl_block.items {
                    if let ImplItem::Fn(method) = impl_item {
                        let name = method.sig.ident.to_string();
                        let line = method.sig.ident.span().start().line;
                        self.analyze_function(&name, line, &method.sig.inputs, &method.block, "Method");
                    }
                }
            }
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    for item in items {
                        self.analyze_item(item);
                    }
                }
            }
            _ => {}
        }
    }

    fn analyze_function(
        &mut self,
        name: &str,
        line: usize,
        inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
        block: &Block,
        unit_type: &str,
    ) {
        let metrics = compute_rust_function_metrics(inputs, block);

        if metrics.statements > self.config.statements_per_function {
            self.violations.push(Violation {
                file: self.file.clone(),
                line,
                unit_name: name.to_string(),
                metric: String::from("statements_per_function"),
                value: metrics.statements,
                threshold: self.config.statements_per_function,
                message: format!("{} '{}' has {} statements (threshold: {})", unit_type, name, metrics.statements, self.config.statements_per_function),
                suggestion: String::from("Break into smaller, focused functions."),
            });
        }

        if metrics.arguments > self.config.arguments_per_function {
            self.violations.push(Violation {
                file: self.file.clone(),
                line,
                unit_name: name.to_string(),
                metric: String::from("arguments_per_function"),
                value: metrics.arguments,
                threshold: self.config.arguments_per_function,
                message: format!("{} '{}' has {} arguments (threshold: {})", unit_type, name, metrics.arguments, self.config.arguments_per_function),
                suggestion: String::from("Group related arguments into a struct."),
            });
        }

        if metrics.max_indentation > self.config.max_indentation_depth {
            self.violations.push(Violation {
                file: self.file.clone(),
                line,
                unit_name: name.to_string(),
                metric: String::from("max_indentation_depth"),
                value: metrics.max_indentation,
                threshold: self.config.max_indentation_depth,
                message: format!("{} '{}' has indentation depth {} (threshold: {})", unit_type, name, metrics.max_indentation, self.config.max_indentation_depth),
                suggestion: String::from("Use early returns, guard clauses, or extract helper functions."),
            });
        }

        if metrics.returns > self.config.returns_per_function {
            self.violations.push(Violation {
                file: self.file.clone(),
                line,
                unit_name: name.to_string(),
                metric: String::from("returns_per_function"),
                value: metrics.returns,
                threshold: self.config.returns_per_function,
                message: format!("{} '{}' has {} return statements (threshold: {})", unit_type, name, metrics.returns, self.config.returns_per_function),
                suggestion: String::from("Reduce exit points; consider restructuring logic."),
            });
        }

        if metrics.branches > self.config.branches_per_function {
            self.violations.push(Violation {
                file: self.file.clone(),
                line,
                unit_name: name.to_string(),
                metric: String::from("branches_per_function"),
                value: metrics.branches,
                threshold: self.config.branches_per_function,
                message: format!("{} '{}' has {} branches (threshold: {})", unit_type, name, metrics.branches, self.config.branches_per_function),
                suggestion: String::from("Consider using match guards, early returns, or extracting logic."),
            });
        }

        if metrics.local_variables > self.config.local_variables_per_function {
            self.violations.push(Violation {
                file: self.file.clone(),
                line,
                unit_name: name.to_string(),
                metric: String::from("local_variables_per_function"),
                value: metrics.local_variables,
                threshold: self.config.local_variables_per_function,
                message: format!("{} '{}' has {} local variables (threshold: {})", unit_type, name, metrics.local_variables, self.config.local_variables_per_function),
                suggestion: String::from("Extract logic into helper functions with fewer variables each."),
            });
        }

        if metrics.cyclomatic_complexity > self.config.cyclomatic_complexity {
            self.violations.push(Violation {
                file: self.file.clone(),
                line,
                unit_name: name.to_string(),
                metric: String::from("cyclomatic_complexity"),
                value: metrics.cyclomatic_complexity,
                threshold: self.config.cyclomatic_complexity,
                message: format!("{} '{}' has cyclomatic complexity {} (threshold: {})", unit_type, name, metrics.cyclomatic_complexity, self.config.cyclomatic_complexity),
                suggestion: String::from("Simplify control flow; extract helper functions."),
            });
        }

        if metrics.nested_function_depth > self.config.nested_function_depth {
            self.violations.push(Violation {
                file: self.file.clone(),
                line,
                unit_name: name.to_string(),
                metric: String::from("nested_closure_depth"),
                value: metrics.nested_function_depth,
                threshold: self.config.nested_function_depth,
                message: format!("{} '{}' has nested closure depth {} (threshold: {})", unit_type, name, metrics.nested_function_depth, self.config.nested_function_depth),
                suggestion: String::from("Extract nested closures into separate functions."),
            });
        }
    }
}

fn get_impl_type_name(impl_block: &syn::ItemImpl) -> Option<String> {
    if let syn::Type::Path(type_path) = impl_block.self_ty.as_ref() {
        type_path.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

/// Compute LCOM (Lack of Cohesion of Methods) for a Rust impl block
/// Returns a value between 0.0 (cohesive) and 1.0 (no cohesion)
pub fn compute_rust_lcom(impl_block: &syn::ItemImpl) -> f64 {
    use std::collections::HashSet;
    
    // Collect fields accessed by each method via self.field
    let mut method_fields: Vec<HashSet<String>> = Vec::new();
    
    for impl_item in &impl_block.items {
        if let ImplItem::Fn(method) = impl_item {
            let fields = extract_self_field_accesses(&method.block);
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

/// Extract all self.field accesses from a block
fn extract_self_field_accesses(block: &Block) -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    
    struct FieldAccessVisitor {
        fields: HashSet<String>,
    }
    
    impl<'ast> Visit<'ast> for FieldAccessVisitor {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if let Expr::Field(field_expr) = expr {
                // Check if base is `self`
                if let Expr::Path(path_expr) = &*field_expr.base {
                    if path_expr.path.is_ident("self") {
                        if let syn::Member::Named(ident) = &field_expr.member {
                            self.fields.insert(ident.to_string());
                        }
                    }
                }
            }
            syn::visit::visit_expr(self, expr);
        }
    }
    
    let mut visitor = FieldAccessVisitor { fields: HashSet::new() };
    visitor.visit_block(block);
    visitor.fields
}

/// Computes metrics for a Rust file
pub fn compute_rust_file_metrics(parsed: &ParsedRustFile) -> RustFileMetrics {
    let mut types = 0;
    let mut imports = 0;

    for item in &parsed.ast.items {
        match item {
            Item::Struct(_) | Item::Enum(_) => types += 1,
            Item::Use(_) => imports += 1,
            _ => {}
        }
    }

    RustFileMetrics {
        lines: parsed.source.lines().count(),
        types,
        imports,
    }
}

/// Computes metrics for a Rust function
pub fn compute_rust_function_metrics(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    block: &Block,
) -> RustFunctionMetrics {
    let mut metrics = RustFunctionMetrics::default();

    // Count arguments (excluding self)
    metrics.arguments = inputs
        .iter()
        .filter(|arg| !matches!(arg, syn::FnArg::Receiver(_)))
        .count();

    // Analyze block
    let mut visitor = FunctionMetricsVisitor::default();
    visitor.visit_block(block);

    metrics.statements = visitor.statements;
    metrics.max_indentation = visitor.max_depth;
    metrics.returns = visitor.returns;
    metrics.branches = visitor.branches;
    metrics.local_variables = visitor.local_variables;
    metrics.nested_function_depth = visitor.max_closure_depth;
    metrics.cyclomatic_complexity = visitor.complexity + 1; // +1 for the function itself

    metrics
}

#[derive(Default)]
struct FunctionMetricsVisitor {
    statements: usize,
    max_depth: usize,
    current_depth: usize,
    returns: usize,
    branches: usize,
    local_variables: usize,
    complexity: usize,
    max_closure_depth: usize,
    current_closure_depth: usize,
}

impl<'ast> Visit<'ast> for FunctionMetricsVisitor {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        self.statements += 1;

        // Count local variables
        if let Stmt::Local(local) = stmt {
            self.count_pattern_bindings(&local.pat);
        }

        syn::visit::visit_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::If(_) => {
                self.branches += 1;
                self.complexity += 1;
                self.current_depth += 1;
                self.max_depth = self.max_depth.max(self.current_depth);
                syn::visit::visit_expr(self, expr);
                self.current_depth -= 1;
            }
            Expr::Match(m) => {
                // Each arm adds complexity
                self.complexity += m.arms.len().saturating_sub(1);
                self.current_depth += 1;
                self.max_depth = self.max_depth.max(self.current_depth);
                syn::visit::visit_expr(self, expr);
                self.current_depth -= 1;
            }
            Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => {
                self.complexity += 1;
                self.current_depth += 1;
                self.max_depth = self.max_depth.max(self.current_depth);
                syn::visit::visit_expr(self, expr);
                self.current_depth -= 1;
            }
            Expr::Return(_) => {
                self.returns += 1;
                syn::visit::visit_expr(self, expr);
            }
            Expr::Binary(bin) => {
                // && and || add complexity
                if matches!(bin.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
                    self.complexity += 1;
                }
                syn::visit::visit_expr(self, expr);
            }
            Expr::Closure(_) => {
                self.current_closure_depth += 1;
                self.max_closure_depth = self.max_closure_depth.max(self.current_closure_depth);
                syn::visit::visit_expr(self, expr);
                self.current_closure_depth -= 1;
            }
            _ => {
                syn::visit::visit_expr(self, expr);
            }
        }
    }
}

impl FunctionMetricsVisitor {
    fn count_pattern_bindings(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(_) => self.local_variables += 1,
            Pat::Tuple(tuple) => {
                for elem in &tuple.elems {
                    self.count_pattern_bindings(elem);
                }
            }
            Pat::TupleStruct(ts) => {
                for elem in &ts.elems {
                    self.count_pattern_bindings(elem);
                }
            }
            Pat::Struct(s) => {
                for field in &s.fields {
                    self.count_pattern_bindings(&field.pat);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_function(code: &str) -> (syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>, syn::Block) {
        let file: syn::File = syn::parse_str(code).expect("should parse");
        if let syn::Item::Fn(func) = &file.items[0] {
            (func.sig.inputs.clone(), (*func.block).clone())
        } else {
            panic!("Expected function");
        }
    }

    #[test]
    fn counts_arguments() {
        let (inputs, block) = parse_function("fn foo(a: i32, b: String, c: bool) {}");
        let metrics = compute_rust_function_metrics(&inputs, &block);
        assert_eq!(metrics.arguments, 3);
    }

    #[test]
    fn counts_statements() {
        let (inputs, block) = parse_function(r#"
fn foo() {
    let x = 1;
    let y = 2;
    println!("{}", x + y);
}
"#);
        let metrics = compute_rust_function_metrics(&inputs, &block);
        assert!(metrics.statements >= 3, "Expected >= 3 statements, got {}", metrics.statements);
    }

    #[test]
    fn counts_branches() {
        let (inputs, block) = parse_function(r#"
fn foo(x: i32) -> &'static str {
    if x > 0 {
        "positive"
    } else if x < 0 {
        "negative"
    } else {
        "zero"
    }
}
"#);
        let metrics = compute_rust_function_metrics(&inputs, &block);
        // if and else if are branches
        assert!(metrics.branches >= 2, "Expected >= 2 branches, got {}", metrics.branches);
    }

    #[test]
    fn counts_local_variables() {
        let (inputs, block) = parse_function(r#"
fn foo() {
    let a = 1;
    let b = 2;
    let (c, d) = (3, 4);
}
"#);
        let metrics = compute_rust_function_metrics(&inputs, &block);
        // a, b, c, d = 4 variables
        assert_eq!(metrics.local_variables, 4);
    }

    #[test]
    fn computes_cyclomatic_complexity() {
        let (inputs, block) = parse_function(r#"
fn foo(x: i32) {
    if x > 0 {
        for i in 0..x {
            println!("{}", i);
        }
    }
}
"#);
        let metrics = compute_rust_function_metrics(&inputs, &block);
        // Base 1 + if + for = 3
        assert!(metrics.cyclomatic_complexity >= 3, "Expected >= 3, got {}", metrics.cyclomatic_complexity);
    }
}

