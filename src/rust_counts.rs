//! Count-based code metrics for Rust

use crate::config::Config;
use crate::violation::{Violation, ViolationBuilder};
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
}

#[derive(Debug, Default)]
pub struct RustTypeMetrics { pub methods: usize }

#[derive(Debug, Default)]
pub struct RustFileMetrics { pub lines: usize, pub types: usize, pub imports: usize }

#[must_use]
pub fn analyze_rust_file(parsed: &ParsedRustFile, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut analyzer = RustAnalyzer::new(&parsed.path, config, &mut violations);
    analyzer.check_file_metrics(parsed);
    for item in &parsed.ast.items { analyzer.analyze_item(item); }
    violations
}

struct RustAnalyzer<'a> {
    file: &'a Path,
    config: &'a Config,
    violations: &'a mut Vec<Violation>,
}

impl<'a> RustAnalyzer<'a> {
    const fn new(file: &'a Path, config: &'a Config, violations: &'a mut Vec<Violation>) -> Self {
        Self { file, config, violations }
    }

    fn check_file_metrics(&mut self, parsed: &ParsedRustFile) {
        let m = compute_rust_file_metrics(parsed);
        let fname = self.file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let c = self.config;
        if m.lines > c.lines_per_file {
            self.violations.push(self.violation(1, &fname)
                .metric("lines_per_file").value(m.lines).threshold(c.lines_per_file)
                .message(format!("File has {} lines (threshold: {})", m.lines, c.lines_per_file))
                .suggestion("Split into multiple modules with focused responsibilities.").build());
        }
        if m.types > c.classes_per_file {
            self.violations.push(self.violation(1, &fname)
                .metric("types_per_file").value(m.types).threshold(c.classes_per_file)
                .message(format!("File has {} types (threshold: {})", m.types, c.classes_per_file))
                .suggestion("Move types to separate files.").build());
        }
        if m.imports > c.imports_per_file {
            self.violations.push(self.violation(1, &fname)
                .metric("imports_per_file").value(m.imports).threshold(c.imports_per_file)
                .message(format!("File has {} use statements (threshold: {})", m.imports, c.imports_per_file))
                .suggestion("Module may have too many responsibilities. Consider splitting.").build());
        }
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

    fn violation(&self, line: usize, name: &str) -> ViolationBuilder {
        Violation::builder(self.file).line(line).unit_name(name)
    }

    fn check_methods_per_type(&mut self, line: usize, name: &str, count: usize) {
        if count > self.config.methods_per_class {
            self.violations.push(self.violation(line, name)
                .metric("methods_per_type").value(count).threshold(self.config.methods_per_class)
                .message(format!("Type '{}' has {} methods (threshold: {})", name, count, self.config.methods_per_class))
                .suggestion("Extract related methods into a separate type with its own impl.").build());
        }
    }

    fn check_lcom(&mut self, impl_block: &syn::ItemImpl, line: usize, name: &str, method_count: usize) -> usize {
        if method_count <= 1 { return 0; }
        let pct = (compute_rust_lcom(impl_block) * 100.0).round() as usize;
        if pct > self.config.lcom {
            self.violations.push(self.violation(line, name)
                .metric("lcom").value(pct).threshold(self.config.lcom)
                .message(format!("Type '{}' has LCOM of {}% (threshold: {}%)", name, pct, self.config.lcom))
                .suggestion("Methods in this impl don't share fields; consider splitting.").build());
        }
        pct
    }

    fn check_god_class(&mut self, line: usize, name: &str, method_count: usize, lcom_pct: usize) {
        if method_count > 20 && lcom_pct > 50 {
            self.violations.push(self.violation(line, name)
                .metric("god_class").value(1).threshold(0)
                .message(format!("Type '{name}' is a God Class: {method_count} methods + {lcom_pct}% LCOM indicates low cohesion"))
                .suggestion("Break into smaller, focused types with single responsibilities.").build());
        }
    }

    fn analyze_function(&mut self, name: &str, line: usize, inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>, block: &Block, ut: &str) {
        let m = compute_rust_function_metrics(inputs, block);
        let c = self.config;
        macro_rules! chk {
            ($mf:ident, $cf:ident, $metric:literal, $label:literal, $sug:literal) => {
                if m.$mf > c.$cf {
                    self.violations.push(self.violation(line, name)
                        .metric($metric).value(m.$mf).threshold(c.$cf)
                        .message(format!("{} '{}' has {} {} (threshold: {})", ut, name, m.$mf, $label, c.$cf))
                        .suggestion($sug).build());
                }
            };
        }
        chk!(statements, statements_per_function, "statements_per_function", "statements", "Break into smaller, focused functions.");
        chk!(arguments, arguments_per_function, "arguments_per_function", "arguments", "Group related arguments into a struct.");
        chk!(max_indentation, max_indentation_depth, "max_indentation_depth", "indentation depth", "Use early returns, guard clauses, or extract helper functions.");
        chk!(returns, returns_per_function, "returns_per_function", "return statements", "Use early guard returns at the top, then a single main return path.");
        chk!(branches, branches_per_function, "branches_per_function", "branches", "Consider using match guards, early returns, or extracting logic.");
        chk!(local_variables, local_variables_per_function, "local_variables_per_function", "local variables", "Extract logic into helper functions with fewer variables each.");
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

/// Compute LCOM (Lack of Cohesion of Methods) for a Rust impl block.
/// LCOM = `pairs_not_sharing_fields` / `total_pairs`. Returns 0.0 (cohesive) to 1.0 (no cohesion).
pub fn compute_rust_lcom(impl_block: &syn::ItemImpl) -> f64 {
    use std::collections::HashSet;
    const MIN_METHODS_FOR_LCOM: usize = 2;
    
    let fields_per_method: Vec<HashSet<String>> = impl_block.items.iter()
        .filter_map(|item| match item {
            ImplItem::Fn(method) => Some(extract_self_field_accesses(&method.block)),
            _ => None,
        })
        .collect();
    
    if fields_per_method.len() < MIN_METHODS_FOR_LCOM {
        return 0.0;
    }
    
    let (pairs_sharing, pairs_not_sharing) = count_field_sharing_pairs(&fields_per_method);
    let total_pairs = pairs_sharing + pairs_not_sharing;
    
    if total_pairs == 0 { 0.0 } else { pairs_not_sharing as f64 / total_pairs as f64 }
}

fn count_field_sharing_pairs(fields_per_method: &[std::collections::HashSet<String>]) -> (usize, usize) {
    let mut sharing = 0;
    let mut not_sharing = 0;
    for i in 0..fields_per_method.len() {
        for j in (i + 1)..fields_per_method.len() {
            if fields_per_method[i].intersection(&fields_per_method[j]).next().is_some() {
                sharing += 1;
            } else {
                not_sharing += 1;
            }
        }
    }
    (sharing, not_sharing)
}

/// Extract all self.field accesses from a block
fn extract_self_field_accesses(block: &Block) -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    
    struct FieldAccessVisitor {
        fields: HashSet<String>,
    }
    
    impl<'ast> Visit<'ast> for FieldAccessVisitor {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if let Expr::Field(field_expr) = expr
                && let Expr::Path(path_expr) = &*field_expr.base
                && path_expr.path.is_ident("self")
                && let syn::Member::Named(ident) = &field_expr.member
            {
                self.fields.insert(ident.to_string());
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

    let non_self_args = inputs.iter().filter(|arg| !matches!(arg, syn::FnArg::Receiver(_)));
    metrics.arguments = non_self_args.count();

    let mut visitor = FunctionMetricsVisitor::default();
    visitor.visit_block(block);

    metrics.statements = visitor.statements;
    metrics.max_indentation = visitor.max_depth;
    metrics.returns = visitor.returns;
    metrics.branches = visitor.branches;
    metrics.local_variables = visitor.local_variables;
    metrics.nested_function_depth = visitor.max_closure_depth;

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

    const fn exit_block(&mut self) { self.current_depth -= 1; }
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

    fn parse_fn(code: &str) -> (syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>, syn::Block) {
        let f: syn::File = syn::parse_str(code).unwrap();
        if let syn::Item::Fn(func) = &f.items[0] { (func.sig.inputs.clone(), (*func.block).clone()) }
        else { panic!("Expected function") }
    }

    #[test]
    fn test_function_metrics() {
        let (i1, b1) = parse_fn("fn foo(a: i32, b: String, c: bool) {}");
        assert_eq!(compute_rust_function_metrics(&i1, &b1).arguments, 3);
        let (i2, b2) = parse_fn("fn f() { let x=1; let y=2; println!(\"{}\",x+y); }");
        assert!(compute_rust_function_metrics(&i2, &b2).statements >= 3);
        let (i3, b3) = parse_fn("fn f(x: i32) { if x>0 {} else if x<0 {} }");
        assert!(compute_rust_function_metrics(&i3, &b3).branches >= 2);
        let (i4, b4) = parse_fn("fn f() { let a=1; let b=2; let (c,d)=(3,4); }");
        assert_eq!(compute_rust_function_metrics(&i4, &b4).local_variables, 4);
    }

    #[test]
    fn test_structs_and_analyzer() {
        let _ = RustFunctionMetrics { statements: 1, arguments: 2, max_indentation: 3, returns: 4, branches: 5, local_variables: 6, nested_function_depth: 8 };
        let _ = (RustTypeMetrics { methods: 5 }, RustFileMetrics { lines: 100, types: 3, imports: 5 });
        let mut viols = Vec::new(); let p = std::path::PathBuf::from("test.rs");
        let cfg = Config::default();
        {
            let analyzer = RustAnalyzer::new(&p, &cfg, &mut viols);
            analyzer.violations.push(analyzer.violation(1, "n").metric("m").value(10).threshold(5).message("msg").suggestion("s").build());
        }
        assert_eq!(viols.len(), 1);
    }

    #[test]
    fn test_analyze_file_and_impl() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "fn foo() {{}}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        assert!(analyze_rust_file(&parsed, &Config::default()).is_empty());
        let f: syn::File = syn::parse_str("impl Foo { fn a(&self) {} fn b(&self) {} }").unwrap();
        if let syn::Item::Impl(i) = &f.items[0] { assert_eq!(count_impl_methods(i), 2); }
        let f2: syn::File = syn::parse_str("impl S { fn a(&self) {} }").unwrap();
        if let syn::Item::Impl(i) = &f2.items[0] { assert!(get_impl_type_name(i).is_some()); }
    }

    #[test]
    fn test_lcom_and_file_metrics() {
        use std::io::Write;
        let f: syn::File = syn::parse_str("struct S { x: i32 } impl S { fn a(&self) { let _=self.x; } }").unwrap();
        if let syn::Item::Impl(i) = &f.items[1] { assert!(compute_rust_lcom(i) <= 1.0); }
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "use std::io;\nstruct A {{}}\nstruct B {{}}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let m = compute_rust_file_metrics(&parsed);
        assert!(m.lines >= 3 && m.types == 2 && m.imports == 1);
    }

    #[test]
    fn test_visitor() {
        use syn::visit::Visit;
        let mut v = FunctionMetricsVisitor::default();
        v.enter_block(); assert_eq!(v.current_depth, 1); v.exit_block();
        let f: syn::File = syn::parse_str("fn f() { let x=1; let y=2; }").unwrap();
        if let syn::Item::Fn(func) = &f.items[0] { for s in &func.block.stmts { v.visit_stmt(s); } }
        assert!(v.statements >= 2);
        let e: syn::Expr = syn::parse_str("if true { 1 } else { 2 }").unwrap();
        let mut v2 = FunctionMetricsVisitor::default(); v2.visit_expr(&e);
        assert!(v2.branches >= 1);
        let f2: syn::File = syn::parse_str("fn f() { let (a,b,c)=(1,2,3); }").unwrap();
        if let syn::Item::Fn(func) = &f2.items[0]
            && let syn::Stmt::Local(l) = &func.block.stmts[0] {
                let mut v3 = FunctionMetricsVisitor::default(); v3.count_pattern_bindings(&l.pat);
                assert_eq!(v3.local_variables, 3);
            }
    }

    #[test]
    fn test_analyzer() {
        let p = std::path::PathBuf::from("t.rs"); let mut v = Vec::new(); let cfg = Config::default();
        let mut a = RustAnalyzer::new(&p, &cfg, &mut v);
        let f: syn::File = syn::parse_str("fn foo() {}").unwrap();
        a.analyze_item(&f.items[0]);
        let f2: syn::File = syn::parse_str("impl Foo { fn bar(&self) {} }").unwrap();
        if let syn::Item::Impl(i) = &f2.items[0] { a.analyze_impl_block(i); }
        let f3: syn::File = syn::parse_str("fn test_fn() { let x=1; }").unwrap();
        if let syn::Item::Fn(func) = &f3.items[0] { a.analyze_function("test_fn", 1, &func.sig.inputs, &func.block, "Fn"); }
    }

    #[test]
    fn test_checks_and_lcom() {
        let p = std::path::PathBuf::from("t.rs"); let mut cfg = Config::default(); cfg.methods_per_class = 5;
        let mut v1 = Vec::new(); RustAnalyzer::new(&p, &cfg, &mut v1).check_methods_per_type(1, "S", 10); assert_eq!(v1.len(), 1);
        let mut v2 = Vec::new(); RustAnalyzer::new(&p, &Config::default(), &mut v2).check_god_class(1, "Big", 25, 75); assert_eq!(v2.len(), 1);
        let mut v3 = Vec::new(); RustAnalyzer::new(&p, &Config::default(), &mut v3).check_god_class(1, "Small", 5, 75); assert!(v3.is_empty());
        let f: syn::File = syn::parse_str("struct S { x: i32, y: i32 } impl S { fn a(&self) { let _=self.x; } fn b(&self) { let _=self.y; } }").unwrap();
        if let syn::Item::Impl(i) = &f.items[1] { let mut cfg2 = Config::default(); cfg2.lcom = 0;
            let mut v4 = Vec::new(); RustAnalyzer::new(&p, &cfg2, &mut v4).check_lcom(i, 1, "S", 2); let _ = v4; }
        let f2: syn::File = syn::parse_str("fn foo() { let _=self.x; }").unwrap();
        if let syn::Item::Fn(func) = &f2.items[0] { let _ = extract_self_field_accesses(&func.block); }
    }

    #[test]
    fn test_count_field_sharing_pairs() {
        use std::collections::HashSet;
        let m1: HashSet<String> = ["x"].into_iter().map(String::from).collect();
        let m2: HashSet<String> = ["x"].into_iter().map(String::from).collect();
        let m3: HashSet<String> = ["y"].into_iter().map(String::from).collect();
        let (sharing, not_sharing) = count_field_sharing_pairs(&[m1, m2, m3]);
        assert_eq!(sharing, 1);
        assert_eq!(not_sharing, 2);
    }

    // --- Violation-triggering tests for Rust file-level metrics ---

    #[test]
    fn test_lines_per_file_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        // Write enough lines to exceed threshold
        for i in 0..50 {
            writeln!(tmp, "// line {i}").unwrap();
        }
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        
        let mut config = Config::default();
        config.lines_per_file = 10; // Set low threshold
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_lines_violation = violations.iter()
            .any(|v| v.metric == "lines_per_file");
        assert!(has_lines_violation,
            "should trigger lines_per_file violation when file has 50 lines > threshold 10");
    }

    #[test]
    fn test_types_per_file_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "struct A {{}}\nstruct B {{}}\nstruct C {{}}\nstruct D {{}}\nstruct E {{}}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        
        let mut config = Config::default();
        config.classes_per_file = 2; // types_per_file threshold
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_types_violation = violations.iter()
            .any(|v| v.metric == "types_per_file");
        assert!(has_types_violation,
            "should trigger types_per_file violation when file has 5 types > threshold 2");
    }

    #[test]
    fn test_imports_per_file_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "use std::io;\nuse std::fs;\nuse std::path;\nuse std::env;\nuse std::collections;").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        
        let mut config = Config::default();
        config.imports_per_file = 2;
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_imports_violation = violations.iter()
            .any(|v| v.metric == "imports_per_file");
        assert!(has_imports_violation,
            "should trigger imports_per_file violation when file has 5 imports > threshold 2");
    }

    // --- Violation-triggering tests for Rust function-level metrics ---

    #[test]
    fn test_statements_per_function_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        // Function with many statements
        let mut code = String::from("fn big_fn() {\n");
        for i in 0..30 {
            code.push_str(&format!("    let x{i} = {i};\n"));
        }
        code.push_str("}\n");
        write!(tmp, "{code}").unwrap();
        
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let mut config = Config::default();
        config.statements_per_function = 10;
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_stmts_violation = violations.iter()
            .any(|v| v.metric == "statements_per_function");
        assert!(has_stmts_violation,
            "should trigger statements_per_function violation when fn has 30 stmts > threshold 10");
    }

    #[test]
    fn test_arguments_per_function_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "fn many_args(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32) {{}}").unwrap();
        
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let mut config = Config::default();
        config.arguments_per_function = 3;
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_args_violation = violations.iter()
            .any(|v| v.metric == "arguments_per_function");
        assert!(has_args_violation,
            "should trigger arguments_per_function violation when fn has 8 args > threshold 3");
    }

    #[test]
    fn test_max_indentation_depth_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        // Deeply nested code
        writeln!(tmp, r"
fn deeply_nested() {{
    if true {{
        if true {{
            if true {{
                if true {{
                    if true {{
                        let x = 1;
                    }}
                }}
            }}
        }}
    }}
}}
").unwrap();
        
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let mut config = Config::default();
        config.max_indentation_depth = 2;
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_indent_violation = violations.iter()
            .any(|v| v.metric == "max_indentation_depth");
        assert!(has_indent_violation,
            "should trigger max_indentation_depth violation for deeply nested code");
    }

    #[test]
    fn test_returns_per_function_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, r"
fn many_returns(x: i32) -> i32 {{
    if x == 1 {{ return 1; }}
    if x == 2 {{ return 2; }}
    if x == 3 {{ return 3; }}
    if x == 4 {{ return 4; }}
    if x == 5 {{ return 5; }}
    if x == 6 {{ return 6; }}
    0
}}
").unwrap();
        
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let mut config = Config::default();
        config.returns_per_function = 2;
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_returns_violation = violations.iter()
            .any(|v| v.metric == "returns_per_function");
        assert!(has_returns_violation,
            "should trigger returns_per_function violation when fn has 6 returns > threshold 2");
    }

    #[test]
    fn test_branches_per_function_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, r"
fn many_branches(x: i32) {{
    if x == 1 {{ }}
    if x == 2 {{ }}
    if x == 3 {{ }}
    if x == 4 {{ }}
    if x == 5 {{ }}
    if x == 6 {{ }}
    if x == 7 {{ }}
    if x == 8 {{ }}
}}
").unwrap();
        
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let mut config = Config::default();
        config.branches_per_function = 3;
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_branches_violation = violations.iter()
            .any(|v| v.metric == "branches_per_function");
        assert!(has_branches_violation,
            "should trigger branches_per_function violation when fn has 8 branches > threshold 3");
    }

    #[test]
    fn test_local_variables_per_function_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        let mut code = String::from("fn many_vars() {\n");
        for i in 0..25 {
            code.push_str(&format!("    let var{i} = {i};\n"));
        }
        code.push_str("}\n");
        write!(tmp, "{code}").unwrap();
        
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let mut config = Config::default();
        config.local_variables_per_function = 5;
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_vars_violation = violations.iter()
            .any(|v| v.metric == "local_variables_per_function");
        assert!(has_vars_violation,
            "should trigger local_variables_per_function violation when fn has 25 vars > threshold 5");
    }

    #[test]
    fn test_nested_closure_depth_violation() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, r"
fn nested_closures() {{
    let f1 = || {{
        let f2 = || {{
            let f3 = || {{
                let f4 = || {{
                    1
                }};
            }};
        }};
    }};
}}
").unwrap();
        
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let mut config = Config::default();
        config.nested_function_depth = 1;
        
        let violations = analyze_rust_file(&parsed, &config);
        
        let has_nested_violation = violations.iter()
            .any(|v| v.metric == "nested_closure_depth");
        assert!(has_nested_violation,
            "should trigger nested_closure_depth violation for deeply nested closures");
    }
}


