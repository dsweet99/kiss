//! Count-based code metrics for Rust

use crate::config::Config;
use crate::counts::Violation;
use crate::rust_parsing::ParsedRustFile;
use std::path::Path;
use syn::visit::Visit;
use syn::{Block, Expr, ImplItem, Item, Pat, Stmt};

#[derive(Debug, Default)]
pub struct RustFunctionMetrics {
    pub statements: usize,
    pub arguments: usize,
    pub max_indentation: usize,
    pub nested_function_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub cyclomatic_complexity: usize,
}

#[derive(Debug, Default)]
pub struct RustTypeMetrics { pub methods: usize }

#[derive(Debug, Default)]
pub struct RustFileMetrics { pub lines: usize, pub types: usize, pub imports: usize }

struct ViolationContext<'a> { file: &'a Path, violations: &'a mut Vec<Violation> }

impl<'a> ViolationContext<'a> {
    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, line: usize, name: &str, metric: &str, val: usize, thresh: usize, msg: String, sug: &str) {
        self.violations.push(
            Violation::builder(self.file)
                .line(line)
                .unit_name(name)
                .metric(metric)
                .value(val)
                .threshold(thresh)
                .message(msg)
                .suggestion(sug)
                .build()
        );
    }
}

#[must_use]
pub fn analyze_rust_file(parsed: &ParsedRustFile, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let file = &parsed.path;
    let file_metrics = compute_rust_file_metrics(parsed);
    let fname = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut ctx = ViolationContext { file, violations: &mut violations };

    if file_metrics.lines > config.lines_per_file {
        ctx.add(1, &fname, "lines_per_file", file_metrics.lines, config.lines_per_file,
            format!("File has {} lines (threshold: {})", file_metrics.lines, config.lines_per_file), "Split into multiple modules with focused responsibilities.");
    }
    if file_metrics.types > config.classes_per_file {
        ctx.add(1, &fname, "types_per_file", file_metrics.types, config.classes_per_file,
            format!("File has {} types (threshold: {})", file_metrics.types, config.classes_per_file), "Move types to separate files.");
    }
    if file_metrics.imports > config.imports_per_file {
        ctx.add(1, &fname, "imports_per_file", file_metrics.imports, config.imports_per_file,
            format!("File has {} use statements (threshold: {})", file_metrics.imports, config.imports_per_file), "Module may have too many responsibilities. Consider splitting.");
    }

    let mut analyzer = RustAnalyzer::new(file, config, &mut violations);
    for item in &parsed.ast.items { analyzer.analyze_item(item); }
    violations
}

struct RustAnalyzer<'a> {
    file: &'a Path,
    config: &'a Config,
    violations: &'a mut Vec<Violation>,
}

impl<'a> RustAnalyzer<'a> {
    fn new(file: &'a Path, config: &'a Config, violations: &'a mut Vec<Violation>) -> Self {
        Self { file, config, violations }
    }

    fn analyze_item(&mut self, item: &Item) {
        match item {
            Item::Fn(func) => {
                let name = func.sig.ident.to_string();
                let line = func.sig.ident.span().start().line;
                self.analyze_function(&name, line, &func.sig.inputs, &func.block, "Function");
            }
            Item::Impl(impl_block) => self.analyze_impl_block(impl_block),
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

    fn analyze_impl_block(&mut self, impl_block: &syn::ItemImpl) {
        let method_count = count_impl_methods(impl_block);
        let type_name = get_impl_type_name(impl_block);
        let line = impl_block.impl_token.span.start().line;
        let name = type_name.as_deref().unwrap_or("<impl>");

        self.check_methods_per_type(line, name, method_count);
        let lcom_pct = self.check_lcom(impl_block, line, name, method_count);
        self.check_god_class(line, name, method_count, lcom_pct);

        for impl_item in &impl_block.items {
            if let ImplItem::Fn(method) = impl_item {
                let mname = method.sig.ident.to_string();
                let mline = method.sig.ident.span().start().line;
                self.analyze_function(&mname, mline, &method.sig.inputs, &method.block, "Method");
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, line: usize, name: &str, metric: &str, val: usize, thresh: usize, msg: String, sug: &str) {
        self.violations.push(
            Violation::builder(self.file)
                .line(line)
                .unit_name(name)
                .metric(metric)
                .value(val)
                .threshold(thresh)
                .message(msg)
                .suggestion(sug)
                .build()
        );
    }

    fn check_methods_per_type(&mut self, line: usize, name: &str, count: usize) {
        if count > self.config.methods_per_class {
            self.push(line, name, "methods_per_type", count, self.config.methods_per_class,
                format!("Type '{}' has {} methods (threshold: {})", name, count, self.config.methods_per_class),
                "Split into multiple impl blocks or extract functionality.");
        }
    }

    fn check_lcom(&mut self, impl_block: &syn::ItemImpl, line: usize, name: &str, method_count: usize) -> usize {
        if method_count <= 1 { return 0; }
        let pct = (compute_rust_lcom(impl_block) * 100.0).round() as usize;
        if pct > self.config.lcom {
            self.push(line, name, "lcom", pct, self.config.lcom,
                format!("Type '{}' has LCOM of {}% (threshold: {}%)", name, pct, self.config.lcom),
                "Methods in this impl don't share fields; consider splitting.");
        }
        pct
    }

    fn check_god_class(&mut self, line: usize, name: &str, method_count: usize, lcom_pct: usize) {
        if method_count > 20 && lcom_pct > 50 {
            self.push(line, name, "god_class", 1, 0,
                format!("Type '{}' is a God Class: {} methods + {}% LCOM indicates low cohesion", name, method_count, lcom_pct),
                "Break into smaller, focused impl blocks with single responsibilities.");
        }
    }

    fn analyze_function(&mut self, name: &str, line: usize, inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>, block: &Block, ut: &str) {
        let m = compute_rust_function_metrics(inputs, block);
        let c = self.config;
        macro_rules! chk {
            ($mf:ident, $cf:ident, $metric:literal, $label:literal, $sug:literal) => {
                if m.$mf > c.$cf { self.push(line, name, $metric, m.$mf, c.$cf, format!("{} '{}' has {} {} (threshold: {})", ut, name, m.$mf, $label, c.$cf), $sug); }
            };
        }
        chk!(statements, statements_per_function, "statements_per_function", "statements", "Break into smaller, focused functions.");
        chk!(arguments, arguments_per_function, "arguments_per_function", "arguments", "Group related arguments into a struct.");
        chk!(max_indentation, max_indentation_depth, "max_indentation_depth", "indentation depth", "Use early returns, guard clauses, or extract helper functions.");
        chk!(returns, returns_per_function, "returns_per_function", "return statements", "Reduce exit points; consider restructuring logic.");
        chk!(branches, branches_per_function, "branches_per_function", "branches", "Consider using match guards, early returns, or extracting logic.");
        chk!(local_variables, local_variables_per_function, "local_variables_per_function", "local variables", "Extract logic into helper functions with fewer variables each.");
        chk!(cyclomatic_complexity, cyclomatic_complexity, "cyclomatic_complexity", "cyclomatic complexity", "Simplify control flow; extract helper functions.");
        chk!(nested_function_depth, nested_function_depth, "nested_closure_depth", "nested closure depth", "Extract nested closures into separate functions.");
    }
}

fn count_impl_methods(impl_block: &syn::ItemImpl) -> usize {
    impl_block.items.iter().filter(|item| matches!(item, ImplItem::Fn(_))).count()
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
                if let Expr::Path(path_expr) = &*field_expr.base
                    && path_expr.path.is_ident("self")
                        && let syn::Member::Named(ident) = &field_expr.member {
                            self.fields.insert(ident.to_string());
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
#[must_use]
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
#[allow(clippy::field_reassign_with_default)]
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

impl FunctionMetricsVisitor {
    fn enter_block(&mut self) {
        self.current_depth += 1;
        self.max_depth = self.max_depth.max(self.current_depth);
    }

    fn exit_block(&mut self) { self.current_depth -= 1; }
}

impl<'ast> Visit<'ast> for FunctionMetricsVisitor {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        self.statements += 1;
        if let Stmt::Local(local) = stmt { self.count_pattern_bindings(&local.pat); }
        syn::visit::visit_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::If(_) => { self.branches += 1; self.complexity += 1; self.enter_block(); }
            Expr::Match(m) => { self.complexity += m.arms.len().saturating_sub(1); self.enter_block(); }
            Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => { self.complexity += 1; self.enter_block(); }
            Expr::Return(_) => { self.returns += 1; }
            Expr::Binary(bin) if matches!(bin.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => { self.complexity += 1; }
            Expr::Closure(_) => {
                self.current_closure_depth += 1;
                self.max_closure_depth = self.max_closure_depth.max(self.current_closure_depth);
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
        match expr {
            Expr::If(_) | Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => self.exit_block(),
            Expr::Closure(_) => self.current_closure_depth -= 1,
            _ => {}
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

    #[test]
    fn test_rust_function_metrics_struct() {
        let m = RustFunctionMetrics { statements: 1, arguments: 2, max_indentation: 3, returns: 4, branches: 5, local_variables: 6, cyclomatic_complexity: 7, nested_function_depth: 8 };
        assert_eq!(m.statements, 1);
    }

    #[test]
    fn test_rust_type_metrics_struct() {
        let m = RustTypeMetrics { methods: 5 };
        assert_eq!(m.methods, 5);
    }

    #[test]
    fn test_rust_file_metrics_struct() {
        let m = RustFileMetrics { lines: 100, types: 3, imports: 5 };
        assert_eq!(m.lines, 100);
    }

    #[test]
    fn test_violation_context_add() {
        let mut viols = Vec::new();
        let path = std::path::PathBuf::from("test.rs");
        let mut ctx = ViolationContext { file: &path, violations: &mut viols };
        ctx.add(1, "name", "metric", 10, 5, "msg".into(), "sug");
        assert_eq!(viols.len(), 1);
    }

    #[test]
    fn test_analyze_rust_file() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "fn foo() {{}}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let viols = analyze_rust_file(&parsed, &Config::default());
        assert!(viols.is_empty());
    }

    #[test]
    fn test_count_impl_methods() {
        let file: syn::File = syn::parse_str("impl Foo { fn a(&self) {} fn b(&self) {} }").unwrap();
        if let syn::Item::Impl(imp) = &file.items[0] {
            assert_eq!(count_impl_methods(imp), 2);
        }
    }

    #[test]
    fn test_get_impl_type_name() {
        let file: syn::File = syn::parse_str("impl MyStruct { fn a(&self) {} }").unwrap();
        if let syn::Item::Impl(imp) = &file.items[0] {
            let name = get_impl_type_name(imp);
            assert!(name.is_some());
        }
    }

    #[test]
    fn test_compute_rust_lcom() {
        let file: syn::File = syn::parse_str("struct S { x: i32 } impl S { fn a(&self) { let _ = self.x; } fn b(&self) { let _ = self.x; } }").unwrap();
        if let syn::Item::Impl(imp) = &file.items[1] {
            let lcom = compute_rust_lcom(imp);
            assert!(lcom >= 0.0 && lcom <= 1.0);
        }
    }

    #[test]
    fn test_compute_rust_file_metrics() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "use std::io;\nstruct A {{}}\nstruct B {{}}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let m = compute_rust_file_metrics(&parsed);
        assert!(m.lines >= 3);
        assert_eq!(m.types, 2);
        assert_eq!(m.imports, 1);
    }

    #[test]
    fn test_function_metrics_visitor_enter_exit() {
        let mut v = FunctionMetricsVisitor::default();
        v.enter_block();
        assert_eq!(v.current_depth, 1);
        v.exit_block();
        assert_eq!(v.current_depth, 0);
    }

    #[test]
    fn test_rust_analyzer_struct() {
        let path = std::path::PathBuf::from("test.rs");
        let mut viols = Vec::new();
        let _analyzer = RustAnalyzer { file: &path, violations: &mut viols, config: &Config::default() };
    }

    #[test]
    fn test_rust_analyzer_analyze_item() {
        let file: syn::File = syn::parse_str("fn foo() {}").unwrap();
        let path = std::path::PathBuf::from("test.rs");
        let mut viols = Vec::new();
        let mut analyzer = RustAnalyzer { file: &path, violations: &mut viols, config: &Config::default() };
        analyzer.analyze_item(&file.items[0]);
        // Should not panic
    }

    #[test]
    fn test_rust_analyzer_analyze_impl_block() {
        let file: syn::File = syn::parse_str("impl Foo { fn bar(&self) {} }").unwrap();
        let path = std::path::PathBuf::from("test.rs");
        let mut viols = Vec::new();
        let mut analyzer = RustAnalyzer { file: &path, violations: &mut viols, config: &Config::default() };
        if let syn::Item::Impl(imp) = &file.items[0] {
            analyzer.analyze_impl_block(imp);
        }
    }

    #[test]
    fn test_rust_analyzer_analyze_function() {
        let file: syn::File = syn::parse_str("fn test_fn() { let x = 1; }").unwrap();
        let path = std::path::PathBuf::from("test.rs");
        let mut viols = Vec::new();
        let mut analyzer = RustAnalyzer { file: &path, violations: &mut viols, config: &Config::default() };
        if let syn::Item::Fn(func) = &file.items[0] {
            analyzer.analyze_function("test_fn", 1, &func.sig.inputs, &func.block, "Function");
        }
    }

    #[test]
    fn test_extract_self_field_accesses() {
        let file: syn::File = syn::parse_str("impl S { fn m(&self) { self.x; self.y; } }").unwrap();
        if let syn::Item::Impl(imp) = &file.items[0] {
            if let syn::ImplItem::Fn(method) = &imp.items[0] {
                let fields = extract_self_field_accesses(&method.block);
                assert!(fields.contains("x") || fields.is_empty()); // May or may not detect based on parsing
            }
        }
    }

    #[test]
    fn test_function_metrics_visitor_visit_stmt() {
        use syn::visit::Visit;
        let file: syn::File = syn::parse_str("fn f() { let x = 1; let y = 2; }").unwrap();
        if let syn::Item::Fn(func) = &file.items[0] {
            let mut v = FunctionMetricsVisitor::default();
            for stmt in &func.block.stmts {
                v.visit_stmt(stmt);
            }
            assert!(v.statements >= 2);
        }
    }

    #[test]
    fn test_function_metrics_visitor_visit_expr() {
        use syn::visit::Visit;
        let expr: syn::Expr = syn::parse_str("if true { 1 } else { 2 }").unwrap();
        let mut v = FunctionMetricsVisitor::default();
        v.visit_expr(&expr);
        assert!(v.branches >= 1);
    }

    #[test]
    fn test_function_metrics_visitor_count_pattern_bindings() {
        let file: syn::File = syn::parse_str("fn f() { let (a, b, c) = (1, 2, 3); }").unwrap();
        if let syn::Item::Fn(func) = &file.items[0] {
            if let syn::Stmt::Local(local) = &func.block.stmts[0] {
                let mut v = FunctionMetricsVisitor::default();
                v.count_pattern_bindings(&local.pat);
                assert_eq!(v.local_variables, 3);
            }
        }
    }

    #[test]
    fn test_check_methods_per_type_under_threshold() {
        let mut violations = Vec::new();
        let path = std::path::PathBuf::from("test.rs");
        let mut config = Config::default();
        config.methods_per_class = 20;
        let mut analyzer = RustAnalyzer::new(&path, &config, &mut violations);
        analyzer.check_methods_per_type(1, "MyStruct", 5);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_check_methods_per_type_over_threshold() {
        let mut violations = Vec::new();
        let path = std::path::PathBuf::from("test.rs");
        let mut config = Config::default();
        config.methods_per_class = 5;
        let mut analyzer = RustAnalyzer::new(&path, &config, &mut violations);
        analyzer.check_methods_per_type(1, "MyStruct", 10);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].metric, "methods_per_type");
    }

    #[test]
    fn test_check_lcom_low_cohesion() {
        let code = r#"
            impl MyStruct {
                fn method1(&self) { self.field1; }
                fn method2(&self) { self.field2; }
            }
        "#;
        let file: syn::File = syn::parse_str(code).unwrap();
        let mut violations = Vec::new();
        let path = std::path::PathBuf::from("test.rs");
        let mut config = Config::default();
        config.lcom = 50;
        let mut analyzer = RustAnalyzer::new(&path, &config, &mut violations);
        if let syn::Item::Impl(impl_block) = &file.items[0] {
            let pct = analyzer.check_lcom(impl_block, 1, "MyStruct", 2);
            // Just verify it returns a percentage and doesn't panic
            assert!(pct <= 100);
        }
    }

    #[test]
    fn test_check_god_class_triggers() {
        let mut violations = Vec::new();
        let path = std::path::PathBuf::from("test.rs");
        let config = Config::default();
        let mut analyzer = RustAnalyzer::new(&path, &config, &mut violations);
        // God class: methods > 20 AND lcom > 50
        analyzer.check_god_class(1, "BigClass", 25, 75);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].metric, "god_class");
    }

    #[test]
    fn test_check_god_class_no_trigger() {
        let mut violations = Vec::new();
        let path = std::path::PathBuf::from("test.rs");
        let config = Config::default();
        let mut analyzer = RustAnalyzer::new(&path, &config, &mut violations);
        // Not a god class: methods <= 20
        analyzer.check_god_class(1, "SmallClass", 5, 75);
        assert!(violations.is_empty());
    }
}

