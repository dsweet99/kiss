
use std::path::Path;
use syn::{Block, ImplItem, Item};

use crate::config::Config;
use crate::rust_fn_metrics::{compute_rust_file_metrics, compute_rust_function_metrics};
use crate::rust_parsing::ParsedRustFile;
use crate::violation::{Violation, ViolationBuilder};

pub use crate::rust_fn_metrics::{RustFileMetrics, RustFunctionMetrics, RustTypeMetrics};

#[must_use]
pub fn analyze_rust_file(parsed: &ParsedRustFile, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut analyzer = RustAnalyzer::new(&parsed.path, config, &mut violations);
    analyzer.check_file_metrics(parsed);
    for item in &parsed.ast.items {
        analyzer.analyze_item(item);
    }
    violations
}

struct RustAnalyzer<'a> {
    file: &'a Path,
    config: &'a Config,
    violations: &'a mut Vec<Violation>,
}

impl<'a> RustAnalyzer<'a> {
    const fn new(
        file: &'a Path,
        config: &'a Config,
        violations: &'a mut Vec<Violation>,
    ) -> Self {
        Self { file, config, violations }
    }

    fn check_file_metrics(&mut self, parsed: &ParsedRustFile) {
        let m = compute_rust_file_metrics(parsed);
        let fname = self.file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let c = self.config;

        if m.statements > c.statements_per_file {
            self.violations.push(
                self.violation(1, &fname)
                    .metric("statements_per_file")
                    .value(m.statements)
                    .threshold(c.statements_per_file)
                    .message(format!("File has {} statements (threshold: {})", m.statements, c.statements_per_file))
                    .suggestion("Split into multiple modules with focused responsibilities.")
                    .build(),
            );
        }
        if m.interface_types > c.interface_types_per_file {
            self.violations.push(
                self.violation(1, &fname)
                    .metric("interface_types_per_file")
                    .value(m.interface_types)
                    .threshold(c.interface_types_per_file)
                    .message(format!("File has {} interface types (threshold: {})", m.interface_types, c.interface_types_per_file))
                    .suggestion("Move traits into a dedicated module.")
                    .build(),
            );
        }
        if m.concrete_types > c.concrete_types_per_file {
            self.violations.push(
                self.violation(1, &fname)
                    .metric("concrete_types_per_file")
                    .value(m.concrete_types)
                    .threshold(c.concrete_types_per_file)
                    .message(format!("File has {} concrete types (threshold: {})", m.concrete_types, c.concrete_types_per_file))
                    .suggestion("Move types to separate files.")
                    .build(),
            );
        }
        // Skip lib.rs and mod.rs - they're module definition files that naturally aggregate re-exports
        if m.imports > c.imported_names_per_file && fname != "lib.rs" && fname != "mod.rs" {
            self.violations.push(
                self.violation(1, &fname)
                    .metric("imported_names_per_file")
                    .value(m.imports)
                    .threshold(c.imported_names_per_file)
                    .message(format!("File has {} use statements (threshold: {})", m.imports, c.imported_names_per_file))
                    .suggestion("Module may have too many responsibilities. Consider splitting.")
                    .build(),
            );
        }
    }

    fn analyze_item(&mut self, item: &Item) {
        match item {
            Item::Fn(func) => {
                let name = func.sig.ident.to_string();
                let line = func.sig.ident.span().start().line;
                self.analyze_function(&name, line, &func.sig.inputs, &func.block, count_non_doc_attrs(&func.attrs), "Function");
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

        for impl_item in &impl_block.items {
            if let ImplItem::Fn(method) = impl_item {
                let mname = method.sig.ident.to_string();
                let mline = method.sig.ident.span().start().line;
                self.analyze_function(&mname, mline, &method.sig.inputs, &method.block, count_non_doc_attrs(&method.attrs), "Method");
            }
        }
    }

    fn violation(&self, line: usize, name: &str) -> ViolationBuilder {
        Violation::builder(self.file).line(line).unit_name(name)
    }

    fn check_methods_per_type(&mut self, line: usize, name: &str, count: usize) {
        if count > self.config.methods_per_class {
            self.violations.push(
                self.violation(line, name)
                    .metric("methods_per_type")
                    .value(count)
                    .threshold(self.config.methods_per_class)
                    .message(format!(
                        "Type '{}' has {} methods (threshold: {})",
                        name, count, self.config.methods_per_class
                    ))
                    .suggestion("Extract related methods into a separate type with its own impl.")
                    .build(),
            );
        }
    }

    fn analyze_function(
        &mut self,
        name: &str,
        line: usize,
        inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
        block: &Block,
        attr_count: usize,
        ut: &str,
    ) {
        let m = compute_rust_function_metrics(inputs, block, attr_count);
        let c = self.config;

        macro_rules! chk {
            ($mf:ident, $cf:ident, $metric:literal, $label:literal, $sug:literal) => {
                if m.$mf > c.$cf {
                    self.violations.push(
                        self.violation(line, name)
                            .metric($metric)
                            .value(m.$mf)
                            .threshold(c.$cf)
                            .message(format!("{} '{}' has {} {} (threshold: {})", ut, name, m.$mf, $label, c.$cf))
                            .suggestion($sug)
                            .build(),
                    );
                }
            };
        }

        chk!(statements, statements_per_function, "statements_per_function", "statements", 
             "Break into smaller, focused functions.");
        chk!(arguments, arguments_per_function, "arguments_per_function", "arguments", 
             "Group related arguments into a struct.");
        chk!(max_indentation, max_indentation_depth, "max_indentation_depth", "indentation depth", 
             "Use early returns, guard clauses, or extract helper functions.");
        chk!(returns, returns_per_function, "returns_per_function", "return statements", 
             "Use early guard returns at the top, then a single main return path.");
        chk!(branches, branches_per_function, "branches_per_function", "branches", 
             "Consider using match guards, early returns, or extracting logic.");
        chk!(local_variables, local_variables_per_function, "local_variables_per_function", "local variables", 
             "Extract logic into helper functions with fewer variables each.");
        chk!(nested_function_depth, nested_function_depth, "nested_closure_depth", "nested closure depth", 
             "Extract nested closures into separate functions.");
        chk!(bool_parameters, boolean_parameters, "bool_parameters", "bool parameters", 
             "Use an enum or a struct with named fields instead of multiple bools.");
        chk!(attributes, annotations_per_function, "attributes_per_function", "attributes", 
             "Consider consolidating attributes or simplifying the function's responsibilities.");
    }
}

/// Count attributes excluding doc comments (#[doc = "..."])
fn count_non_doc_attrs(attrs: &[syn::Attribute]) -> usize {
    attrs.iter().filter(|a| !a.path().is_ident("doc")).count()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_helpers() {
        let f: syn::File = syn::parse_str("impl Foo { fn a(&self) {} fn b(&self) {} }").unwrap();
        if let syn::Item::Impl(i) = &f.items[0] { assert_eq!(count_impl_methods(i), 2); }
        let f2: syn::File = syn::parse_str("impl MyStruct { fn a(&self) {} }").unwrap();
        if let syn::Item::Impl(i) = &f2.items[0] { assert_eq!(get_impl_type_name(i), Some("MyStruct".to_string())); }
    }

    #[test]
    fn test_analyzer_basic() {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "fn foo() {{}}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        assert!(analyze_rust_file(&parsed, &Config::default()).is_empty());
        let p = std::path::PathBuf::from("t.rs");
        let mut v = Vec::new();
        RustAnalyzer::new(&p, &Config::default(), &mut v).analyze_item(&syn::parse_str::<syn::File>("fn foo() {}").unwrap().items[0]);
    }

    #[test]
    fn test_analyzer_checks() {
        let p = std::path::PathBuf::from("t.rs");
        let cfg = Config { methods_per_class: 5, ..Default::default() };
        let mut v = Vec::new();
        RustAnalyzer::new(&p, &cfg, &mut v).check_methods_per_type(1, "S", 10);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn test_analyzer_impl_and_fn() {
        let p = std::path::PathBuf::from("t.rs");
        let fi: syn::File = syn::parse_str("impl Foo { fn bar(&self) { let x = 1; } }").unwrap();
        if let syn::Item::Impl(i) = &fi.items[0] { let mut v = Vec::new(); RustAnalyzer::new(&p, &Config::default(), &mut v).analyze_impl_block(i); }
        let ff: syn::File = syn::parse_str("fn foo(x: i32) { let y = x + 1; }").unwrap();
        if let syn::Item::Fn(func) = &ff.items[0] { let mut v = Vec::new(); RustAnalyzer::new(&p, &Config::default(), &mut v).analyze_function("foo", 1, &func.sig.inputs, &func.block, count_non_doc_attrs(&func.attrs), "Function"); }
    }
}
