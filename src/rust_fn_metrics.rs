
use syn::visit::Visit;
use syn::{Block, Expr, Pat, Stmt};

use crate::rust_parsing::ParsedRustFile;

#[derive(Debug, Default)]
pub struct RustFunctionMetrics {
    pub statements: usize,
    pub arguments: usize,
    pub max_indentation: usize,
    pub nested_function_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub bool_parameters: usize,
    pub attributes: usize,
}

#[derive(Debug, Default)]
pub struct RustTypeMetrics {
    pub methods: usize,
}

#[derive(Debug, Default)]
pub struct RustFileMetrics {
    pub lines: usize,
    pub types: usize,
    pub imports: usize,
}

#[must_use]
pub fn compute_rust_file_metrics(parsed: &ParsedRustFile) -> RustFileMetrics {
    let mut types = 0;
    let mut imports = 0;

    for item in &parsed.ast.items {
        match item {
            syn::Item::Struct(_) | syn::Item::Enum(_) => types += 1,
            syn::Item::Use(_) => imports += 1,
            _ => {}
        }
    }

    RustFileMetrics {
        lines: parsed.source.lines().count(),
        types,
        imports,
    }
}

#[allow(clippy::field_reassign_with_default)]
pub fn compute_rust_function_metrics(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    block: &Block,
    attr_count: usize,
) -> RustFunctionMetrics {
    let mut metrics = RustFunctionMetrics::default();

    let non_self_args: Vec<_> = inputs
        .iter()
        .filter(|arg| !matches!(arg, syn::FnArg::Receiver(_)))
        .collect();
    metrics.arguments = non_self_args.len();
    metrics.bool_parameters = non_self_args.iter().filter(|arg| is_bool_param(arg)).count();
    metrics.attributes = attr_count;

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

fn is_bool_param(arg: &syn::FnArg) -> bool {
    matches!(arg, syn::FnArg::Typed(pt) if matches!(&*pt.ty, syn::Type::Path(tp) if tp.path.is_ident("bool")))
}

#[derive(Default)]
pub struct FunctionMetricsVisitor {
    pub statements: usize,
    pub max_depth: usize,
    pub current_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub complexity: usize,
    pub max_closure_depth: usize,
    pub current_closure_depth: usize,
}

impl FunctionMetricsVisitor {
    pub fn enter_block(&mut self) {
        self.current_depth += 1;
        self.max_depth = self.max_depth.max(self.current_depth);
    }

    pub const fn exit_block(&mut self) {
        self.current_depth -= 1;
    }

    pub fn count_pattern_bindings(&mut self, pat: &Pat) {
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

impl<'ast> Visit<'ast> for FunctionMetricsVisitor {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        self.statements += 1;
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
                self.enter_block();
            }
            Expr::Match(m) => {
                self.complexity += m.arms.len().saturating_sub(1);
                self.enter_block();
            }
            Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => {
                self.complexity += 1;
                self.enter_block();
            }
            Expr::Return(_) => {
                self.returns += 1;
            }
            Expr::Binary(bin) if matches!(bin.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => {
                self.complexity += 1;
            }
            Expr::Closure(_) => {
                self.current_closure_depth += 1;
                self.max_closure_depth = self.max_closure_depth.max(self.current_closure_depth);
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
        match expr {
            Expr::If(_) | Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => {
                self.exit_block();
            }
            Expr::Closure(_) => self.current_closure_depth -= 1,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::visit::Visit;

    fn parse_fn(
        code: &str,
    ) -> (
        syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
        syn::Block,
    ) {
        let f: syn::File = syn::parse_str(code).unwrap();
        if let syn::Item::Fn(func) = &f.items[0] {
            (func.sig.inputs.clone(), (*func.block).clone())
        } else {
            panic!("Expected function")
        }
    }

    #[test]
    fn test_function_metrics() {
        let (i1, b1) = parse_fn("fn foo(a: i32, b: String, c: bool) {}");
        let m1 = compute_rust_function_metrics(&i1, &b1, 0);
        assert_eq!(m1.arguments, 3);
        assert_eq!(m1.bool_parameters, 1);

        let (i2, b2) = parse_fn(r#"fn f() { let x=1; let y=2; println!("{}",x+y); }"#);
        assert!(compute_rust_function_metrics(&i2, &b2, 0).statements >= 3);

        let (i3, b3) = parse_fn("fn f(x: i32) { if x>0 {} else if x<0 {} }");
        assert!(compute_rust_function_metrics(&i3, &b3, 0).branches >= 2);

        let (i4, b4) = parse_fn("fn f() { let a=1; let b=2; let (c,d)=(3,4); }");
        assert_eq!(compute_rust_function_metrics(&i4, &b4, 0).local_variables, 4);

        let (i5, b5) = parse_fn("fn f() {}");
        assert_eq!(compute_rust_function_metrics(&i5, &b5, 3).attributes, 3);
    }

    #[test]
    fn test_visitor() {
        let mut v = FunctionMetricsVisitor::default();
        v.enter_block();
        assert_eq!(v.current_depth, 1);
        v.exit_block();

        let f: syn::File = syn::parse_str("fn f() { let x=1; let y=2; }").unwrap();
        if let syn::Item::Fn(func) = &f.items[0] {
            for s in &func.block.stmts {
                v.visit_stmt(s);
            }
        }
        assert!(v.statements >= 2);

        let e: syn::Expr = syn::parse_str("if true { 1 } else { 2 }").unwrap();
        let mut v2 = FunctionMetricsVisitor::default();
        v2.visit_expr(&e);
        assert!(v2.branches >= 1);

        let f2: syn::File = syn::parse_str("fn f() { let (a,b,c)=(1,2,3); }").unwrap();
        if let syn::Item::Fn(func) = &f2.items[0]
            && let syn::Stmt::Local(l) = &func.block.stmts[0]
        {
            let mut v3 = FunctionMetricsVisitor::default();
            v3.count_pattern_bindings(&l.pat);
            assert_eq!(v3.local_variables, 3);
        }
    }

    #[test]
    fn test_is_bool_param() {
        let f: syn::File = syn::parse_str("fn foo(a: bool, b: i32) {}").unwrap();
        if let syn::Item::Fn(func) = &f.items[0] {
            assert!(is_bool_param(&func.sig.inputs[0]));
            assert!(!is_bool_param(&func.sig.inputs[1]));
        }
    }

    #[test]
    fn test_structs() {
        let _ = RustFunctionMetrics {
            statements: 1,
            arguments: 2,
            max_indentation: 3,
            returns: 4,
            branches: 5,
            local_variables: 6,
            nested_function_depth: 8,
            bool_parameters: 0,
            attributes: 0,
        };
        let _ = (
            RustTypeMetrics { methods: 5 },
            RustFileMetrics {
                lines: 100,
                types: 3,
                imports: 5,
            },
        );
    }

    #[test]
    fn test_file_metrics() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "use std::io;\nstruct A {{}}\nstruct B {{}}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let m = compute_rust_file_metrics(&parsed);
        assert!(m.lines >= 3 && m.types == 2 && m.imports == 1);
    }
}

