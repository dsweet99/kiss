//! Test References for Rust - detect code units that may lack test coverage

use crate::rust_parsing::ParsedRustFile;
use std::collections::HashSet;
use std::path::PathBuf;
use syn::visit::Visit;
use syn::{Attribute, Expr, ImplItem, Item};

/// A code unit definition in Rust
#[derive(Debug, Clone)]
pub struct RustCodeDefinition {
    pub name: String,
    pub kind: &'static str, // "function", "method", "struct", "enum"
    pub file: PathBuf,
    pub line: usize,
}

/// Result of test reference analysis for Rust
#[derive(Debug)]
pub struct RustTestRefAnalysis {
    /// All definitions found in source files
    pub definitions: Vec<RustCodeDefinition>,
    /// Names referenced in test code
    pub test_references: HashSet<String>,
    /// Definitions not referenced by any test
    pub unreferenced: Vec<RustCodeDefinition>,
}

/// Check if a file is a test file based on Rust conventions
pub fn is_rust_test_file(path: &std::path::Path) -> bool {
    // Check for tests/ directory
    if path.components().any(|c| c.as_os_str() == "tests") {
        return true;
    }
    
    // Check for test module files
    if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
        if name.ends_with("_test") || name.starts_with("test_") {
            return true;
        }
    }
    
    false
}

/// Check if an item has a #[test] attribute
fn has_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
    })
}

/// Check if an item has #[cfg(test)] attribute
fn has_cfg_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("cfg") {
            if let Ok(nested) = attr.parse_args::<syn::Ident>() {
                return nested == "test";
            }
        }
        false
    })
}

/// Analyze test references across all parsed Rust files
pub fn analyze_rust_test_refs(parsed_files: &[&ParsedRustFile]) -> RustTestRefAnalysis {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();

    for parsed in parsed_files {
        let is_test = is_rust_test_file(&parsed.path);
        
        // Collect definitions from non-test files
        if !is_test {
            collect_rust_definitions(&parsed.ast, &parsed.path, &mut definitions);
        }
        
        // Collect test references from test files AND #[test] functions
        if is_test {
            collect_rust_references(&parsed.ast, &mut test_references);
        } else {
            // Also collect references from #[cfg(test)] modules and #[test] functions
            collect_test_module_references(&parsed.ast, &mut test_references);
        }
    }

    // Find unreferenced definitions
    let unreferenced = definitions
        .iter()
        .filter(|def| !test_references.contains(&def.name))
        .cloned()
        .collect();

    RustTestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
    }
}

/// Collect function, struct, enum definitions from a Rust file
fn collect_rust_definitions(ast: &syn::File, file: &PathBuf, defs: &mut Vec<RustCodeDefinition>) {
    for item in &ast.items {
        collect_definitions_from_item(item, file, defs);
    }
}

fn collect_definitions_from_item(item: &Item, file: &PathBuf, defs: &mut Vec<RustCodeDefinition>) {
    match item {
        Item::Fn(func) => {
            // Skip test functions and private functions
            if !has_test_attribute(&func.attrs) && !func.sig.ident.to_string().starts_with('_') {
                defs.push(RustCodeDefinition {
                    name: func.sig.ident.to_string(),
                    kind: "function",
                    file: file.clone(),
                    line: func.sig.ident.span().start().line,
                });
            }
        }
        Item::Struct(s) => {
            if !s.ident.to_string().starts_with('_') {
                defs.push(RustCodeDefinition {
                    name: s.ident.to_string(),
                    kind: "struct",
                    file: file.clone(),
                    line: s.ident.span().start().line,
                });
            }
        }
        Item::Enum(e) => {
            if !e.ident.to_string().starts_with('_') {
                defs.push(RustCodeDefinition {
                    name: e.ident.to_string(),
                    kind: "enum",
                    file: file.clone(),
                    line: e.ident.span().start().line,
                });
            }
        }
        Item::Impl(impl_block) => {
            // Skip #[cfg(test)] impl blocks
            if has_cfg_test_attribute(&impl_block.attrs) {
                return;
            }
            
            for impl_item in &impl_block.items {
                if let ImplItem::Fn(method) = impl_item {
                    if !has_test_attribute(&method.attrs) && !method.sig.ident.to_string().starts_with('_') {
                        defs.push(RustCodeDefinition {
                            name: method.sig.ident.to_string(),
                            kind: "method",
                            file: file.clone(),
                            line: method.sig.ident.span().start().line,
                        });
                    }
                }
            }
        }
        Item::Mod(m) => {
            // Skip #[cfg(test)] modules
            if has_cfg_test_attribute(&m.attrs) {
                return;
            }
            
            if let Some((_, items)) = &m.content {
                for item in items {
                    collect_definitions_from_item(item, file, defs);
                }
            }
        }
        _ => {}
    }
}

/// Collect all name references from test code
fn collect_rust_references(ast: &syn::File, refs: &mut HashSet<String>) {
    let mut visitor = ReferenceVisitor { refs };
    visitor.visit_file(ast);
}

/// Collect references from #[cfg(test)] modules and #[test] functions within non-test files
fn collect_test_module_references(ast: &syn::File, refs: &mut HashSet<String>) {
    for item in &ast.items {
        match item {
            Item::Mod(m) if has_cfg_test_attribute(&m.attrs) => {
                if let Some((_, items)) = &m.content {
                    let temp_ast = syn::File {
                        shebang: None,
                        attrs: vec![],
                        items: items.clone(),
                    };
                    collect_rust_references(&temp_ast, refs);
                }
            }
            Item::Fn(func) if has_test_attribute(&func.attrs) => {
                let mut visitor = ReferenceVisitor { refs };
                visitor.visit_item_fn(func);
            }
            _ => {}
        }
    }
}

struct ReferenceVisitor<'a> {
    refs: &'a mut HashSet<String>,
}

impl<'ast> Visit<'ast> for ReferenceVisitor<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::Call(call) => {
                // Extract function name from call
                if let Expr::Path(path) = call.func.as_ref() {
                    if let Some(segment) = path.path.segments.last() {
                        self.refs.insert(segment.ident.to_string());
                    }
                }
            }
            Expr::MethodCall(method) => {
                self.refs.insert(method.method.to_string());
            }
            Expr::Struct(s) => {
                // Struct instantiation
                if let Some(segment) = s.path.segments.last() {
                    self.refs.insert(segment.ident.to_string());
                }
            }
            Expr::Path(path) => {
                // Variable/type reference
                if let Some(segment) = path.path.segments.last() {
                    self.refs.insert(segment.ident.to_string());
                }
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }

    fn visit_type(&mut self, ty: &'ast syn::Type) {
        if let syn::Type::Path(type_path) = ty {
            if let Some(segment) = type_path.path.segments.last() {
                self.refs.insert(segment.ident.to_string());
            }
        }
        syn::visit::visit_type(self, ty);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_is_rust_test_file_tests_directory() {
        assert!(is_rust_test_file(Path::new("tests/integration.rs")));
        assert!(is_rust_test_file(Path::new("/some/path/tests/helper.rs")));
        assert!(is_rust_test_file(Path::new("project/tests/mod.rs")));
    }

    #[test]
    fn test_is_rust_test_file_naming_conventions() {
        assert!(is_rust_test_file(Path::new("test_utils.rs")));
        assert!(is_rust_test_file(Path::new("utils_test.rs")));
        assert!(is_rust_test_file(Path::new("src/test_helper.rs")));
    }

    #[test]
    fn test_is_rust_test_file_regular_files() {
        assert!(!is_rust_test_file(Path::new("src/main.rs")));
        assert!(!is_rust_test_file(Path::new("src/lib.rs")));
        assert!(!is_rust_test_file(Path::new("utils.rs")));
        assert!(!is_rust_test_file(Path::new("testing.rs"))); // "testing" != "test_"
    }
}

