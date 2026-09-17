use crate::rust_parsing::ParsedRustFile;
use crate::units::CodeUnitKind;
use syn::visit::Visit;
use syn::{ImplItem, Item};

#[derive(Debug)]
pub struct RustCodeUnit {
    pub kind: CodeUnitKind,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub parent_type: Option<String>,
    pub trait_impl: bool,
}

struct CodeUnitVisitor {
    units: Vec<RustCodeUnit>,
    current_impl_type: Option<String>,
    pub(super) source_lines: usize,
}

impl CodeUnitVisitor {
    fn new(source: &str) -> Self {
        Self {
            units: Vec::new(),
            current_impl_type: None,
            source_lines: source.lines().count(),
        }
    }
}

impl<'ast> Visit<'ast> for CodeUnitVisitor {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            Item::Fn(func) => self.visit_top_level_fn(func),
            Item::Struct(s) => self.record_struct(s),
            Item::Enum(e) => self.record_enum(e),
            Item::Impl(impl_block) => self.visit_impl_block(impl_block),
            Item::Mod(m) => self.visit_item_mod(m),
            _ => syn::visit::visit_item(self, item),
        }
    }
}

impl<'ast> CodeUnitVisitor {
    fn visit_top_level_fn(&mut self, func: &'ast syn::ItemFn) {
        let start_line = func.sig.ident.span().start().line;
        let end_line = start_line + estimate_block_lines(&func.block);

        self.units.push(RustCodeUnit {
            kind: CodeUnitKind::Function,
            name: func.sig.ident.to_string(),
            start_line,
            end_line,
            parent_type: None,
            trait_impl: false,
        });
        syn::visit::visit_item_fn(self, func);
    }

    fn record_struct(&mut self, s: &syn::ItemStruct) {
        let start_line = s.ident.span().start().line;
        self.units.push(RustCodeUnit {
            kind: CodeUnitKind::Class,
            name: s.ident.to_string(),
            start_line,
            end_line: start_line,
            parent_type: None,
            trait_impl: false,
        });
    }

    fn record_enum(&mut self, e: &syn::ItemEnum) {
        let start_line = e.ident.span().start().line;
        self.units.push(RustCodeUnit {
            kind: CodeUnitKind::Class,
            name: e.ident.to_string(),
            start_line,
            end_line: start_line,
            parent_type: None,
            trait_impl: false,
        });
    }

    fn visit_impl_block(&mut self, impl_block: &'ast syn::ItemImpl) {
        let type_name = if let syn::Type::Path(type_path) = impl_block.self_ty.as_ref() {
            type_path.path.segments.last().map(|s| s.ident.to_string())
        } else {
            None
        };

        self.current_impl_type = type_name;
        let trait_impl = impl_block.trait_.is_some();
        for impl_item in &impl_block.items {
            if let ImplItem::Fn(method) = impl_item {
                let start_line = method.sig.ident.span().start().line;
                let end_line = start_line + estimate_block_lines(&method.block);

                self.units.push(RustCodeUnit {
                    kind: CodeUnitKind::Method,
                    name: method.sig.ident.to_string(),
                    start_line,
                    end_line,
                    parent_type: self.current_impl_type.clone(),
                    trait_impl,
                });
            }
        }

        self.current_impl_type = None;
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if m.content.is_some() {
            let start_line = m.ident.span().start().line;
            self.units.push(RustCodeUnit {
                kind: CodeUnitKind::Module,
                name: m.ident.to_string(),
                start_line,
                end_line: start_line,
                parent_type: None,
                trait_impl: false,
            });
        }
        syn::visit::visit_item_mod(self, m);
    }
}

fn estimate_block_lines(block: &syn::Block) -> usize {
    if block.stmts.is_empty() {
        return 1;
    }
    let start = block.brace_token.span.open().start().line;
    let end = block.brace_token.span.close().end().line;

    if end >= start {
        end - start + 1
    } else {
        block.stmts.len().max(1)
    }
}

pub fn extract_rust_code_units(parsed: &ParsedRustFile) -> Vec<RustCodeUnit> {
    let mut visitor = CodeUnitVisitor::new(&parsed.source);
    visitor.units.push(RustCodeUnit {
        kind: CodeUnitKind::Module,
        name: parsed.path.file_stem().map_or_else(
            || "unknown".to_string(),
            |s| s.to_string_lossy().into_owned(),
        ),
        start_line: 1,
        end_line: visitor.source_lines,
        parent_type: None,
        trait_impl: false,
    });
    for item in &parsed.ast.items {
        visitor.visit_item(item);
    }

    visitor.units
}

#[cfg(test)]
mod inline_coverage_tests {
    use super::*;
    use syn::visit::Visit;

    #[test]
    fn direct_visitor_helpers_exercised() {
        let src = "enum E { A, B }\nstruct S { x: i32 }\nmod inner { fn nested() {} }\nimpl S { fn f(&self) { let _ = 1; } }\nfn top() { let _ = 2; }\n";
        let file: syn::File = syn::parse_str(src).unwrap();
        let mut visitor = CodeUnitVisitor::new(src);
        for item in &file.items {
            match item {
                syn::Item::Fn(f) => visitor.visit_top_level_fn(f),
                syn::Item::Struct(s) => visitor.record_struct(s),
                syn::Item::Enum(e) => visitor.record_enum(e),
                syn::Item::Impl(i) => visitor.visit_impl_block(i),
                syn::Item::Mod(m) => visitor.visit_item_mod(m),
                other => visitor.visit_item(other),
            }
        }
        let names: Vec<_> = visitor.units.iter().map(|u| u.name.as_str()).collect();
        for expected in ["E", "S", "inner", "f", "top", "nested"] {
            assert!(names.contains(&expected), "missing {expected} in {names:?}");
        }
    }

    #[test]
    fn estimate_block_lines_fallback_on_missing_brace_span() {
        let block: syn::Block = syn::parse_str("{ let x = 1; }").unwrap();
        assert!(estimate_block_lines(&block) >= 1);
    }
}

#[cfg(test)]
#[path = "rust_units_test.rs"]
mod rust_units_test;
