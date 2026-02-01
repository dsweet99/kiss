use crate::rust_parsing::ParsedRustFile;
use crate::units::CodeUnitKind;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::{Attribute, Expr, ImplItem, Item};

#[derive(Debug, Clone)]
pub struct RustCodeDefinition {
    pub name: String,
    pub kind: CodeUnitKind,
    pub file: PathBuf,
    pub line: usize,
    pub impl_for_type: Option<String>,
}

#[derive(Debug)]
pub struct RustTestRefAnalysis {
    pub definitions: Vec<RustCodeDefinition>,
    pub test_references: HashSet<String>,
    pub unreferenced: Vec<RustCodeDefinition>,
}

fn is_rs_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("rs")
}

fn has_test_naming_pattern(path: &Path) -> bool {
    path.file_stem()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            name.ends_with("_test") || name.starts_with("test_") || name.ends_with("_integration")
        })
}

#[must_use]
pub fn is_rust_test_file(path: &Path) -> bool {
    is_rs_file(path) && has_test_naming_pattern(path)
}

fn has_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test"))
}

fn has_cfg_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test")
    })
}

fn is_directly_referenced(def: &RustCodeDefinition, refs: &HashSet<String>) -> bool {
    refs.contains(&def.name)
}

fn is_trait_impl_with_referenced_type(def: &RustCodeDefinition, refs: &HashSet<String>) -> bool {
    def.kind == CodeUnitKind::TraitImplMethod
        && def.impl_for_type.as_ref().is_some_and(|t| refs.contains(t))
}

fn is_covered_by_tests(def: &RustCodeDefinition, refs: &HashSet<String>) -> bool {
    is_directly_referenced(def, refs) || is_trait_impl_with_referenced_type(def, refs)
}

pub fn analyze_rust_test_refs(parsed_files: &[&ParsedRustFile]) -> RustTestRefAnalysis {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            collect_rust_references(&parsed.ast, &mut test_references);
        } else {
            collect_rust_definitions(&parsed.ast, &parsed.path, &mut definitions);
            collect_test_module_references(&parsed.ast, &mut test_references);
        }
    }
    let unreferenced = definitions
        .iter()
        .filter(|d| !is_covered_by_tests(d, &test_references))
        .cloned()
        .collect();
    RustTestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
    }
}

fn collect_rust_definitions(ast: &syn::File, file: &Path, defs: &mut Vec<RustCodeDefinition>) {
    for item in &ast.items {
        collect_definitions_from_item(item, file, defs);
    }
}

fn is_private(name: &str) -> bool {
    name.starts_with('_')
}

fn try_add_def(
    defs: &mut Vec<RustCodeDefinition>,
    name: &str,
    kind: CodeUnitKind,
    file: &Path,
    line: usize,
    impl_for_type: Option<String>,
) {
    if !is_private(name) {
        defs.push(RustCodeDefinition {
            name: name.to_string(),
            kind,
            file: file.to_path_buf(),
            line,
            impl_for_type,
        });
    }
}

fn extract_type_name(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(p) = ty {
        p.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

fn collect_impl_methods(
    impl_block: &syn::ItemImpl,
    file: &Path,
    defs: &mut Vec<RustCodeDefinition>,
) {
    let is_trait_impl = impl_block.trait_.is_some();
    let impl_type_name = extract_type_name(&impl_block.self_ty);
    for impl_item in &impl_block.items {
        if let ImplItem::Fn(m) = impl_item {
            if has_test_attribute(&m.attrs) {
                continue;
            }
            let (kind, impl_for) = if is_trait_impl {
                (CodeUnitKind::TraitImplMethod, impl_type_name.clone())
            } else {
                (CodeUnitKind::Method, None)
            };
            try_add_def(
                defs,
                &m.sig.ident.to_string(),
                kind,
                file,
                m.sig.ident.span().start().line,
                impl_for,
            );
        }
    }
}

fn collect_definitions_from_item(item: &Item, file: &Path, defs: &mut Vec<RustCodeDefinition>) {
    match item {
        Item::Fn(f) if !has_test_attribute(&f.attrs) => try_add_def(
            defs,
            &f.sig.ident.to_string(),
            CodeUnitKind::Function,
            file,
            f.sig.ident.span().start().line,
            None,
        ),
        Item::Struct(s) => try_add_def(
            defs,
            &s.ident.to_string(),
            CodeUnitKind::Struct,
            file,
            s.ident.span().start().line,
            None,
        ),
        Item::Enum(e) => try_add_def(
            defs,
            &e.ident.to_string(),
            CodeUnitKind::Enum,
            file,
            e.ident.span().start().line,
            None,
        ),
        Item::Impl(i) if !has_cfg_test_attribute(&i.attrs) => collect_impl_methods(i, file, defs),
        Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
            if let Some((_, items)) = &m.content {
                for i in items {
                    collect_definitions_from_item(i, file, defs);
                }
            }
        }
        _ => {}
    }
}

fn collect_rust_references(ast: &syn::File, refs: &mut HashSet<String>) {
    ReferenceVisitor { refs }.visit_file(ast);
}

fn collect_test_module_references(ast: &syn::File, refs: &mut HashSet<String>) {
    for item in &ast.items {
        match item {
            Item::Mod(m) if has_cfg_test_attribute(&m.attrs) => {
                if let Some((_, items)) = &m.content {
                    collect_rust_references(
                        &syn::File {
                            shebang: None,
                            attrs: vec![],
                            items: items.clone(),
                        },
                        refs,
                    );
                }
            }
            Item::Fn(f) if has_test_attribute(&f.attrs) => {
                ReferenceVisitor { refs }.visit_item_fn(f);
            }
            _ => {}
        }
    }
}

struct ReferenceVisitor<'a> {
    refs: &'a mut HashSet<String>,
}

fn is_external_crate(name: &str) -> bool {
    matches!(
        name,
        "std"
            | "core"
            | "alloc"
            | "syn"
            | "proc_macro"
            | "proc_macro2"
            | "quote"
            | "serde"
            | "tokio"
            | "async_std"
            | "futures"
            | "anyhow"
            | "thiserror"
            | "clap"
            | "log"
            | "tracing"
            | "regex"
            | "chrono"
            | "uuid"
            | "rand"
            | "reqwest"
            | "hyper"
            | "axum"
            | "actix"
            | "diesel"
            | "sqlx"
            | "sea_orm"
            | "rocket"
            | "warp"
            | "tide"
            | "petgraph"
            | "tempfile"
            | "ignore"
            | "tree_sitter"
            | "tree_sitter_python"
    )
}

fn starts_with_external_crate(path: &syn::Path) -> bool {
    path.segments
        .first()
        .is_some_and(|s| is_external_crate(&s.ident.to_string()))
}
fn is_rust_keyword(name: &str) -> bool {
    matches!(name, "self" | "Self" | "super" | "crate")
}

fn insert_path_segments(path: &syn::Path, refs: &mut HashSet<String>) {
    if starts_with_external_crate(path) {
        return;
    }
    for seg in &path.segments {
        let name = seg.ident.to_string();
        if !is_rust_keyword(&name) {
            refs.insert(name);
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceVisitor<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::Call(c) => {
                if let Expr::Path(p) = c.func.as_ref() {
                    insert_path_segments(&p.path, self.refs);
                }
            }
            Expr::MethodCall(m) => {
                self.refs.insert(m.method.to_string());
            }
            Expr::Struct(s) => insert_path_segments(&s.path, self.refs),
            Expr::Path(p) => insert_path_segments(&p.path, self.refs),
            Expr::Macro(m) => visit_macro_tokens(&m.mac.tokens, self.refs),
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }
    fn visit_type(&mut self, ty: &'ast syn::Type) {
        if let syn::Type::Path(p) = ty {
            insert_path_segments(&p.path, self.refs);
        }
        syn::visit::visit_type(self, ty);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        visit_macro_tokens(&mac.tokens, self.refs);
        syn::visit::visit_macro(self, mac);
    }
}

struct ExprList(Vec<Expr>);
impl syn::parse::Parse for ExprList {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut exprs = Vec::new();
        while !input.is_empty() {
            exprs.push(input.parse()?);
            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        Ok(Self(exprs))
    }
}

fn try_parse_as_single_expr(tokens: &proc_macro2::TokenStream, refs: &mut HashSet<String>) -> bool {
    if let Ok(e) = syn::parse2::<Expr>(tokens.clone()) {
        ReferenceVisitor { refs }.visit_expr(&e);
        return true;
    }
    false
}
fn try_parse_as_expr_list(tokens: &proc_macro2::TokenStream, refs: &mut HashSet<String>) -> bool {
    if let Ok(ExprList(exprs)) = syn::parse2::<ExprList>(tokens.clone()) {
        for e in exprs {
            ReferenceVisitor { refs }.visit_expr(&e);
        }
        return true;
    }
    false
}
fn visit_nested_token_groups(tokens: &proc_macro2::TokenStream, refs: &mut HashSet<String>) {
    for t in tokens.clone() {
        if let proc_macro2::TokenTree::Group(g) = t {
            visit_macro_tokens(&g.stream(), refs);
        }
    }
}
fn visit_macro_tokens(tokens: &proc_macro2::TokenStream, refs: &mut HashSet<String>) {
    if try_parse_as_single_expr(tokens, refs) {
        return;
    }
    if try_parse_as_expr_list(tokens, refs) {
        return;
    }
    visit_nested_token_groups(tokens, refs);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_parsing::parse_rust_file;
    use std::io::Write;

    #[test]
    fn test_file_detection_and_helpers() {
        assert!(
            is_rust_test_file(Path::new("test_utils.rs"))
                && is_rust_test_file(Path::new("utils_test.rs"))
        );
        assert!(!is_rust_test_file(Path::new("src/main.rs")));
        assert!(is_rs_file(Path::new("foo.rs")) && !is_rs_file(Path::new("foo.py")));
        assert!(
            has_test_naming_pattern(Path::new("test_foo.rs"))
                && !has_test_naming_pattern(Path::new("foo.rs"))
        );
        assert!(is_private("_helper") && !is_private("helper"));
        assert!(is_rust_keyword("self") && !is_rust_keyword("foo"));
        let ty: syn::Type = syn::parse_str("Foo").unwrap();
        assert_eq!(extract_type_name(&ty), Some("Foo".into()));
        let _ = RustTestRefAnalysis {
            definitions: vec![],
            test_references: HashSet::new(),
            unreferenced: vec![],
        };
    }

    #[test]
    fn test_definitions_and_references() {
        let f1: syn::File = syn::parse_str("#[test]\nfn t() {}").unwrap();
        let f2: syn::File = syn::parse_str("#[cfg(test)]\nmod tests {}").unwrap();
        if let syn::Item::Fn(f) = &f1.items[0] {
            assert!(has_test_attribute(&f.attrs));
        }
        if let syn::Item::Mod(m) = &f2.items[0] {
            assert!(has_cfg_test_attribute(&m.attrs));
        }
        let f: syn::File = syn::parse_str("fn foo() {}\nstruct Bar {}").unwrap();
        let mut defs = Vec::new();
        collect_rust_definitions(&f, Path::new("t.rs"), &mut defs);
        assert!(defs.len() >= 2);
        for item in &f.items {
            collect_definitions_from_item(item, Path::new("t.rs"), &mut defs);
        }
        let fi: syn::File = syn::parse_str("impl Foo { fn bar(&self) {} }").unwrap();
        if let Item::Impl(i) = &fi.items[0] {
            collect_impl_methods(i, Path::new("t.rs"), &mut defs);
        }
        let f3: syn::File =
            syn::parse_str("#[cfg(test)] mod tests { fn call_foo() { foo(); } }").unwrap();
        let mut refs = HashSet::new();
        collect_test_module_references(&f3, &mut refs);
        assert!(refs.contains("foo"));
    }

    #[test]
    fn test_coverage_checks() {
        let def = RustCodeDefinition {
            name: "fmt".into(),
            kind: CodeUnitKind::TraitImplMethod,
            file: "t.rs".into(),
            line: 1,
            impl_for_type: Some("MyType".into()),
        };
        let refs: HashSet<String> = ["MyType", "foo"].into_iter().map(String::from).collect();
        assert!(is_trait_impl_with_referenced_type(&def, &refs));
        let def2 = RustCodeDefinition {
            name: "foo".into(),
            kind: CodeUnitKind::Function,
            file: "t.rs".into(),
            line: 1,
            impl_for_type: None,
        };
        assert!(is_directly_referenced(&def2, &refs));
        assert!(is_covered_by_tests(&def, &refs));
        assert!(is_external_crate("std") && !is_external_crate("my_module"));
        let p: syn::Path = syn::parse_str("std::io").unwrap();
        assert!(starts_with_external_crate(&p));
    }

    #[test]
    fn test_visitor_and_macros() {
        let mut refs = HashSet::new();
        let _ = ReferenceVisitor { refs: &mut refs };
        let ty: syn::Type = syn::parse_str("MyType").unwrap();
        ReferenceVisitor { refs: &mut refs }.visit_type(&ty);
        assert!(refs.contains("MyType"));
        let mac: syn::ExprMacro = syn::parse_str("println!(\"test\")").unwrap();
        ReferenceVisitor { refs: &mut refs }.visit_macro(&mac.mac);
        let el: ExprList = syn::parse_str("a, b, c").unwrap();
        assert_eq!(el.0.len(), 3);
        let tokens1: proc_macro2::TokenStream = "foo()".parse().unwrap();
        assert!(try_parse_as_single_expr(&tokens1, &mut refs));
        let tokens2: proc_macro2::TokenStream = "a, b".parse().unwrap();
        assert!(try_parse_as_expr_list(&tokens2, &mut refs));
        let tokens3: proc_macro2::TokenStream = "{ bar() }".parse().unwrap();
        visit_nested_token_groups(&tokens3, &mut refs);
        let tokens4: proc_macro2::TokenStream = "baz()".parse().unwrap();
        visit_macro_tokens(&tokens4, &mut refs);
    }

    #[test]
    fn test_analyze_refs() {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        write!(
            tmp,
            "fn foo() {{}}\n#[cfg(test)] mod tests {{ use super::*; #[test] fn t() {{ foo(); }} }}"
        )
        .unwrap();
        let parsed = parse_rust_file(tmp.path()).unwrap();
        let analysis = analyze_rust_test_refs(&[&parsed]);
        assert!(!analysis.definitions.is_empty());
    }

    #[test]
    fn test_collect_rust_references() {
        let ast: syn::File = syn::parse_str("fn test() { foo(); bar::baz(); }").unwrap();
        let mut refs = HashSet::new();
        collect_rust_references(&ast, &mut refs);
        assert!(refs.contains("foo"));
    }

    #[test]
    fn test_insert_path_segments() {
        let path: syn::Path = syn::parse_str("foo::bar::Baz").unwrap();
        let mut refs = HashSet::new();
        insert_path_segments(&path, &mut refs);
        assert!(refs.contains("foo"));
        assert!(refs.contains("bar"));
        assert!(refs.contains("Baz"));
        let std_path: syn::Path = syn::parse_str("std::io::Read").unwrap();
        insert_path_segments(&std_path, &mut refs);
        assert!(!refs.contains("io"));
    }
}
