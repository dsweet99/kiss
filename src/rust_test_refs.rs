use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
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

use crate::test_refs::CoveringTest;

type PerTestUsage = Vec<(PathBuf, Vec<(String, HashSet<String>)>)>;

#[derive(Debug)]
pub struct RustTestRefAnalysis {
    pub definitions: Vec<RustCodeDefinition>,
    pub test_references: HashSet<String>,
    pub unreferenced: Vec<RustCodeDefinition>,
    /// For each covered definition (file, name), the list of tests that reference it.
    pub coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>>,
}

fn is_rs_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
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
    is_rs_file(path)
        && (has_test_naming_pattern(path) || crate::test_refs::is_in_test_directory(path))
}

fn has_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test"))
}

fn cfg_contains_test(tokens: proc_macro2::TokenStream) -> bool {
    let mut iter = tokens.into_iter();
    while let Some(token) = iter.next() {
        match &token {
            proc_macro2::TokenTree::Ident(ident) if ident == "test" => return true,
            proc_macro2::TokenTree::Ident(ident) if ident == "not" => {
                let _ = iter.next();
            }
            proc_macro2::TokenTree::Ident(ident) if *ident == "all" || *ident == "any" => {
                if let Some(proc_macro2::TokenTree::Group(group)) = iter.next()
                    && cfg_contains_test(group.stream())
                {
                    return true;
                }
            }
            proc_macro2::TokenTree::Group(group) => {
                if cfg_contains_test(group.stream()) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn has_cfg_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        if let syn::Meta::List(ref list) = a.meta {
            return cfg_contains_test(list.tokens.clone());
        }
        false
    })
}

fn is_directly_referenced(
    def: &RustCodeDefinition,
    refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
) -> bool {
    if !refs.contains(&def.name) {
        return false;
    }
    let unique = name_files.get(&def.name).is_none_or(|f| f.len() <= 1);
    if unique {
        return true;
    }
    if let Some(winner) = disambiguation.get(&def.name) {
        return *winner == def.file;
    }
    false
}

fn is_impl_with_referenced_type(def: &RustCodeDefinition, refs: &HashSet<String>) -> bool {
    matches!(
        def.kind,
        CodeUnitKind::TraitImplMethod | CodeUnitKind::Method
    ) && def.impl_for_type.as_ref().is_some_and(|t| refs.contains(t))
}

fn is_covered_by_tests(
    def: &RustCodeDefinition,
    refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
) -> bool {
    is_directly_referenced(def, refs, name_files, disambiguation)
        || is_impl_with_referenced_type(def, refs)
}

pub fn analyze_rust_test_refs(
    parsed_files: &[&ParsedRustFile],
    graph: Option<&DependencyGraph>,
) -> RustTestRefAnalysis {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();
    let mut per_test_usage: PerTestUsage = Vec::new();
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            collect_rust_references(&parsed.ast, &mut test_references);
        } else {
            collect_rust_definitions(&parsed.ast, &parsed.path, &mut definitions);
            collect_test_module_references(&parsed.ast, &mut test_references);
        }
        let test_funcs = collect_per_test_usage(&parsed.ast);
        if !test_funcs.is_empty() {
            per_test_usage.push((parsed.path.clone(), test_funcs));
        }
    }
    let name_files = crate::test_refs::build_name_file_map(
        definitions
            .iter()
            .map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let disambiguation = crate::test_refs::build_disambiguation_map(
        &name_files,
        &test_references,
        &per_test_usage,
        graph,
    );
    let unreferenced = definitions
        .iter()
        .filter(|d| !is_covered_by_tests(d, &test_references, &name_files, &disambiguation))
        .cloned()
        .collect();
    let coverage_map = build_rust_coverage_map(
        &definitions,
        &per_test_usage,
        &name_files,
        &disambiguation,
    );
    RustTestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
        coverage_map,
    }
}

#[allow(clippy::type_complexity)]
fn build_rust_coverage_map(
    definitions: &[RustCodeDefinition],
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
) -> HashMap<(PathBuf, String), Vec<CoveringTest>> {
    let mut name_to_defs: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, def) in definitions.iter().enumerate() {
        name_to_defs.entry(&def.name).or_default().push(i);
        if let Some(ref t) = def.impl_for_type {
            name_to_defs.entry(t.as_str()).or_default().push(i);
        }
    }

    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    for (test_path, test_funcs) in per_test_usage {
        for (test_id, usage_refs) in test_funcs {
            if test_id.is_empty() {
                continue;
            }
            let mut seen = HashSet::new();
            for ref_name in usage_refs {
                let Some(def_indices) = name_to_defs.get(ref_name.as_str()) else {
                    continue;
                };
                for &idx in def_indices {
                    if !seen.insert(idx) {
                        continue;
                    }
                    let def = &definitions[idx];
                    if !is_covered_by_tests(def, usage_refs, name_files, disambiguation) {
                        continue;
                    }
                    let key = (def.file.clone(), def.name.clone());
                    let entry = (test_path.clone(), test_id.clone());
                    let list = coverage_map.entry(key).or_default();
                    if !list.contains(&entry) {
                        list.push(entry);
                    }
                }
            }
        }
    }
    coverage_map
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
                (CodeUnitKind::Method, impl_type_name.clone())
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
            CodeUnitKind::Class,
            file,
            s.ident.span().start().line,
            None,
        ),
        Item::Enum(e) => try_add_def(
            defs,
            &e.ident.to_string(),
            CodeUnitKind::Class,
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

/// Collects references from a single function body. Returns the set of referenced names.
fn collect_rust_references_for_fn(f: &syn::ItemFn) -> HashSet<String> {
    let mut refs = HashSet::new();
    ReferenceVisitor { refs: &mut refs }.visit_item_fn(f);
    refs
}

/// Collects per-test (`test_id`, `usage_refs`) from a file.
/// `test_id` format: `fn_name` for top-level `#[test]` fn, `mod_name::fn_name` for `#[cfg(test)]` mod.
fn collect_per_test_usage(ast: &syn::File) -> Vec<(String, HashSet<String>)> {
    let mut out = Vec::new();
    collect_per_test_usage_from_items(&ast.items, "", &mut out);
    out
}

fn collect_per_test_usage_from_items(
    items: &[syn::Item],
    prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
) {
    for item in items {
        match item {
            Item::Mod(m) if has_cfg_test_attribute(&m.attrs) => {
                let mod_name = m.ident.to_string();
                let mod_prefix = if prefix.is_empty() {
                    mod_name.clone()
                } else {
                    format!("{prefix}::{mod_name}")
                };
                if let Some((_, mod_items)) = &m.content {
                    collect_per_test_usage_from_items(mod_items, &mod_prefix, out);
                }
            }
            Item::Fn(f) if has_test_attribute(&f.attrs) => {
                let fn_name = f.sig.ident.to_string();
                let refs = collect_rust_references_for_fn(f);
                let test_id = if prefix.is_empty() {
                    fn_name
                } else {
                    format!("{prefix}::{fn_name}")
                };
                out.push((test_id, refs));
            }
            _ => {}
        }
    }
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
            | "serde_json"
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
            | "rayon"
            | "itertools"
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
            is_rs_file(Path::new("foo.RS")),
            ".RS extension must match Rust (Path::extension preserves case)"
        );
        assert!(
            is_rust_test_file(Path::new("bar_test.RS")),
            "Rust test file detection must accept uppercase .RS"
        );
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
            coverage_map: HashMap::new(),
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
        assert!(is_impl_with_referenced_type(&def, &refs));
        let def2 = RustCodeDefinition {
            name: "foo".into(),
            kind: CodeUnitKind::Function,
            file: "t.rs".into(),
            line: 1,
            impl_for_type: None,
        };
        let all_definitions = [def.clone(), def2.clone()];
        let name_files = crate::test_refs::build_name_file_map(
            all_definitions
                .iter()
                .map(|d| (d.name.as_str(), d.file.as_path())),
        );
        let disambiguation =
            crate::test_refs::build_disambiguation_map(&name_files, &refs, &[], None);
        assert!(is_directly_referenced(
            &def2,
            &refs,
            &name_files,
            &disambiguation
        ));
        assert!(is_covered_by_tests(
            &def,
            &refs,
            &name_files,
            &disambiguation
        ));
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
        let analysis = analyze_rust_test_refs(&[&parsed], None);
        assert!(!analysis.definitions.is_empty());
        let key = (parsed.path, "foo".to_string());
        assert!(
            analysis.coverage_map.contains_key(&key),
            "coverage_map should contain foo from #[cfg(test)] mod"
        );
        let covering = &analysis.coverage_map[&key];
        assert!(
            covering.iter().any(|(_, f)| f == "tests::t"),
            "foo should be covered by tests::t, got {covering:?}"
        );
    }

    #[test]
    fn test_collect_rust_references() {
        let ast: syn::File = syn::parse_str("fn test() { foo(); bar::baz(); }").unwrap();
        let mut refs = HashSet::new();
        collect_rust_references(&ast, &mut refs);
        assert!(refs.contains("foo"));
    }

    // === Bug-hunting tests ===

    #[test]
    fn test_is_external_crate_common_deps() {
        // Common Rust ecosystem crates should be recognized as external.
        assert!(
            is_external_crate("rayon"),
            "rayon should be recognized as external crate"
        );
        assert!(
            is_external_crate("serde_json"),
            "serde_json should be recognized as external crate"
        );
        assert!(
            is_external_crate("itertools"),
            "itertools should be recognized as external crate"
        );
    }

    #[test]
    fn test_same_name_different_files_disambiguated_by_module() {
        let tmp = tempfile::TempDir::new().unwrap();

        let alpha_path = tmp.path().join("alpha.rs");
        std::fs::write(&alpha_path, "pub fn helper() {}").unwrap();

        let beta_path = tmp.path().join("beta.rs");
        std::fs::write(&beta_path, "pub fn helper() {}").unwrap();

        let test_path = tmp.path().join("test_alpha.rs");
        std::fs::write(&test_path, "fn t() { alpha::helper(); }").unwrap();

        let parsed_alpha = parse_rust_file(&alpha_path).unwrap();
        let parsed_beta = parse_rust_file(&beta_path).unwrap();
        let parsed_test = parse_rust_file(&test_path).unwrap();

        let analysis = analyze_rust_test_refs(&[&parsed_alpha, &parsed_beta, &parsed_test], None);

        assert_eq!(analysis.definitions.len(), 2, "both files define helper()");

        let alpha_uncovered = analysis.unreferenced.iter().any(|d| d.file == alpha_path);
        assert!(
            !alpha_uncovered,
            "alpha::helper should be covered (test imports from alpha)"
        );

        let beta_uncovered = analysis.unreferenced.iter().any(|d| d.file == beta_path);
        assert!(
            beta_uncovered,
            "beta::helper should be uncovered (no test references beta)"
        );
    }

    #[test]
    fn test_impl_method_covered_when_type_referenced() {
        let tmp = tempfile::TempDir::new().unwrap();

        let alpha_path = tmp.path().join("alpha.rs");
        std::fs::write(
            &alpha_path,
            "pub struct Foo {}\nimpl Foo {\n    pub fn new() -> Self { Foo {} }\n}\n",
        )
        .unwrap();

        let beta_path = tmp.path().join("beta.rs");
        std::fs::write(
            &beta_path,
            "pub struct Bar {}\nimpl Bar {\n    pub fn new() -> Self { Bar {} }\n}\n",
        )
        .unwrap();

        let test_path = tmp.path().join("test_alpha.rs");
        std::fs::write(&test_path, "fn t() { let _f = Foo::new(); }").unwrap();

        let parsed_alpha = parse_rust_file(&alpha_path).unwrap();
        let parsed_beta = parse_rust_file(&beta_path).unwrap();
        let parsed_test = parse_rust_file(&test_path).unwrap();

        let analysis = analyze_rust_test_refs(&[&parsed_alpha, &parsed_beta, &parsed_test], None);

        let uncovered: Vec<_> = analysis
            .unreferenced
            .iter()
            .map(|d| (d.name.as_str(), d.file.to_str().unwrap()))
            .collect();
        assert!(
            !analysis
                .unreferenced
                .iter()
                .any(|d| d.name == "new" && d.file == alpha_path),
            "Foo::new should be covered (test calls Foo::new()), but unreferenced: {uncovered:?}"
        );
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

    #[test]
    fn test_touch_for_static_test_coverage() {
        fn touch<T>(_: T) {}
        touch(cfg_contains_test);
        touch(build_rust_coverage_map);
        touch(collect_rust_references_for_fn);
        touch(collect_per_test_usage);
        touch(collect_per_test_usage_from_items);
    }
}
