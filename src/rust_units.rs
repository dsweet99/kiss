//! Code unit extraction from Rust ASTs

use crate::rust_parsing::ParsedRustFile;
use crate::units::CodeUnitKind;
use syn::visit::Visit;
use syn::{ImplItem, Item};

/// A code unit extracted from Rust source
#[derive(Debug)]
pub struct RustCodeUnit {
    pub kind: CodeUnitKind,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    /// For methods: the impl type name (e.g., "MyStruct")
    pub parent_type: Option<String>,
}

/// Visitor to extract code units from Rust AST
struct CodeUnitVisitor {
    units: Vec<RustCodeUnit>,
    current_impl_type: Option<String>,
    source_lines: usize,
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
            Item::Fn(func) => {
                let start_line = func.sig.ident.span().start().line;
                // Approximate end line from block
                let end_line = start_line + estimate_block_lines(&func.block);
                
                self.units.push(RustCodeUnit {
                    kind: CodeUnitKind::Function,
                    name: func.sig.ident.to_string(),
                    start_line,
                    end_line,
                    parent_type: None,
                });
                
                // Visit nested items
                syn::visit::visit_item_fn(self, func);
            }
            Item::Struct(s) => {
                let start_line = s.ident.span().start().line;
                self.units.push(RustCodeUnit {
                    kind: CodeUnitKind::Class, // struct maps to Class
                    name: s.ident.to_string(),
                    start_line,
                    end_line: start_line, // Approximate
                    parent_type: None,
                });
            }
            Item::Enum(e) => {
                let start_line = e.ident.span().start().line;
                self.units.push(RustCodeUnit {
                    kind: CodeUnitKind::Class, // enum maps to Class
                    name: e.ident.to_string(),
                    start_line,
                    end_line: start_line,
                    parent_type: None,
                });
            }
            Item::Impl(impl_block) => {
                // Get the type name being implemented
                let type_name = if let syn::Type::Path(type_path) = impl_block.self_ty.as_ref() {
                    type_path
                        .path
                        .segments
                        .last()
                        .map(|s| s.ident.to_string())
                } else {
                    None
                };
                
                self.current_impl_type = type_name;
                
                // Visit methods in the impl block
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
                        });
                    }
                }
                
                self.current_impl_type = None;
            }
            Item::Mod(m) => {
                if m.content.is_some() {
                    // Inline module
                    let start_line = m.ident.span().start().line;
                    self.units.push(RustCodeUnit {
                        kind: CodeUnitKind::Module,
                        name: m.ident.to_string(),
                        start_line,
                        end_line: start_line, // Will be updated
                        parent_type: None,
                    });
                }
                syn::visit::visit_item_mod(self, m);
            }
            _ => {
                syn::visit::visit_item(self, item);
            }
        }
    }
}

/// Estimate the number of lines in a block (rough approximation)
fn estimate_block_lines(block: &syn::Block) -> usize {
    if block.stmts.is_empty() {
        return 1;
    }
    
    // Use span info if available
    let start = block.brace_token.span.open().start().line;
    let end = block.brace_token.span.close().end().line;
    
    if end >= start {
        end - start + 1
    } else {
        block.stmts.len().max(1)
    }
}

/// Extracts all code units from a parsed Rust file
pub fn extract_rust_code_units(parsed: &ParsedRustFile) -> Vec<RustCodeUnit> {
    let mut visitor = CodeUnitVisitor::new(&parsed.source);
    
    // Add the module itself as a code unit
    visitor.units.push(RustCodeUnit {
        kind: CodeUnitKind::Module,
        name: parsed
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string()),
        start_line: 1,
        end_line: visitor.source_lines,
        parent_type: None,
    });
    
    // Visit all items in the file
    for item in &parsed.ast.items {
        visitor.visit_item(item);
    }
    
    visitor.units
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_parsing::parse_rust_file;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn parse_and_extract(code: &str) -> Vec<RustCodeUnit> {
        let mut file = NamedTempFile::with_suffix(".rs").unwrap();
        write!(file, "{}", code).unwrap();
        let parsed = parse_rust_file(file.path()).expect("should parse");
        extract_rust_code_units(&parsed)
    }

    #[test]
    fn extracts_function() {
        let units = parse_and_extract("fn hello() {}");
        
        let functions: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Function).collect();
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "hello");
    }

    #[test]
    fn extracts_struct_and_methods() {
        let units = parse_and_extract(r#"
struct Counter { value: i32 }

impl Counter {
    fn new() -> Self { Counter { value: 0 } }
    fn get(&self) -> i32 { self.value }
}
"#);
        
        let structs: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Class).collect();
        let methods: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Method).collect();
        
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name, "Counter");
        
        assert_eq!(methods.len(), 2);
        assert!(methods.iter().any(|m| m.name == "new"));
        assert!(methods.iter().any(|m| m.name == "get"));
    }

    #[test]
    fn extracts_enum() {
        let units = parse_and_extract("enum Color { Red, Green, Blue }");
        
        let enums: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Class).collect();
        assert_eq!(enums.len(), 1);
        assert_eq!(enums[0].name, "Color");
    }

    #[test]
    fn includes_module_for_file() {
        let units = parse_and_extract("fn foo() {}");
        
        let modules: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Module).collect();
        assert!(!modules.is_empty(), "Should have at least one module (the file)");
    }

    #[test]
    fn test_code_unit_visitor_struct() {
        let visitor = CodeUnitVisitor::new("fn foo() {}\n");
        assert!(visitor.source_lines >= 1);
    }

    #[test]
    fn test_visit_item_directly() {
        use syn::visit::Visit;
        let file: syn::File = syn::parse_str("fn bar() {}").unwrap();
        let mut visitor = CodeUnitVisitor::new("fn bar() {}\n");
        visitor.visit_item(&file.items[0]);
        assert!(visitor.units.iter().any(|u| u.name == "bar"));
    }

    #[test]
    fn test_estimate_block_lines() {
        let file: syn::File = syn::parse_str("fn f() { let x = 1; let y = 2; }").unwrap();
        if let syn::Item::Fn(func) = &file.items[0] {
            let lines = estimate_block_lines(&func.block);
            assert!(lines >= 1);
        }
    }
}

