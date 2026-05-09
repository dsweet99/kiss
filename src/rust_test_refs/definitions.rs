use super::{has_cfg_test_attribute, has_test_attribute};
use crate::units::CodeUnitKind;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use syn::{Expr, ImplItem, Item, Stmt};

use super::references::{ReferenceVisitor, collect_rust_references};
use syn::visit::Visit;

/// Returns true if the file path is a Rust binary entry point.
///
/// Excludes paths that contain a **normal** path component named exactly `tests` (Cargo’s
/// integration-test tree), not substring matches — so e.g. `legacy_tests/src/main.rs` is still
/// treated as an entry point.
pub(super) fn is_binary_entry_point(path: &Path) -> bool {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::Normal(s) if s == "tests"))
    {
        return false;
    }
    let path_str = path.to_string_lossy();
    if path.file_name().is_some_and(|n| n == "main.rs") {
        return true;
    }
    path_str.contains("src/bin/") || path_str.contains("src\\bin\\")
}

/// Well-known constructors that don't need qualification.
/// These are standard library types used in typical main error handling.
fn is_well_known_constructor(name: &str) -> bool {
    matches!(name, "Ok" | "Err" | "Some" | "None" | "Box" | "Vec")
}

/// Returns true if the expression is a qualified call (has a module path)
/// or a well-known constructor like `Ok`, `Err`, `Some`, `None`, and every argument is
/// structurally trivial (so work cannot hide in call arguments).
/// Examples: `lib::run()`, `crate::foo()`, `Ok(())`
fn is_qualified_or_known_call(expr: &Expr) -> bool {
    match expr {
        Expr::Call(c) => {
            if let Expr::Path(p) = c.func.as_ref() {
                let callee_ok = if p.path.segments.len() >= 2 {
                    true
                } else if p.path.segments.len() == 1 {
                    let name = p.path.segments[0].ident.to_string();
                    is_well_known_constructor(&name)
                } else {
                    false
                };
                callee_ok && c.args.iter().all(is_trivial_expr)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Returns true if the expression is structurally trivial (just delegation).
/// Recursively checks if/match arms, blocks, etc.
fn is_trivial_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Call(_) => is_qualified_or_known_call(expr),
        // `Expr::Macro` and other unlisted shapes: not analyzed as delegation (see wildcard).
        Expr::Path(_) | Expr::Lit(_) => true,
        Expr::Return(r) => r.expr.as_ref().is_none_or(|e| is_trivial_expr(e)),
        Expr::Try(t) => is_trivial_expr(&t.expr),
        Expr::Await(a) => is_trivial_expr(&a.base),
        Expr::Block(b) => is_delegation_only_block(&b.block),
        Expr::If(i) => {
            is_trivial_expr(&i.cond)
                && is_delegation_only_block(&i.then_branch)
                && i.else_branch
                    .as_ref()
                    .is_none_or(|(_, e)| is_trivial_expr(e))
        }
        Expr::Match(m) => {
            is_trivial_expr(&m.expr)
                && m.arms.iter().all(|arm| {
                    arm.guard.as_ref().is_none_or(|(_, g)| is_trivial_expr(g))
                        && is_trivial_expr(&arm.body)
                })
        }
        Expr::Let(l) => is_trivial_expr(&l.expr),
        Expr::MethodCall(m) => is_trivial_expr(&m.receiver) && m.args.iter().all(is_trivial_expr),
        Expr::Field(f) => is_trivial_expr(&f.base),
        Expr::Reference(r) => is_trivial_expr(&r.expr),
        Expr::Unary(u) => is_trivial_expr(&u.expr),
        Expr::Binary(b) => is_trivial_expr(&b.left) && is_trivial_expr(&b.right),
        Expr::Paren(p) => is_trivial_expr(&p.expr),
        Expr::Tuple(t) => t.elems.iter().all(is_trivial_expr),
        _ => false,
    }
}

/// Returns true if the statement is trivial (delegation only, no local definitions).
fn is_trivial_stmt(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(e, _) => is_trivial_expr(e),
        Stmt::Local(l) => l.init.as_ref().is_none_or(|i| is_trivial_expr(&i.expr)),
        Stmt::Item(_) | Stmt::Macro(_) => false,
    }
}

/// Returns true if the block only contains delegation (qualified calls, simple control flow).
/// Macro bodies are not analyzed and are not treated as delegation. No local function/struct/enum definitions.
pub(super) fn is_delegation_only_block(block: &syn::Block) -> bool {
    block.stmts.iter().all(is_trivial_stmt)
}

/// Returns true if the function is a trivial binary entry point that only delegates.
/// Such functions are excluded from coverage requirements since they cannot be
/// directly tested (main cannot be called from tests) and contain no real logic.
pub(super) fn is_trivial_binary_main(f: &syn::ItemFn, path: &Path) -> bool {
    if f.sig.ident != "main" {
        return false;
    }
    if !f.sig.inputs.is_empty() {
        return false;
    }
    if !is_binary_entry_point(path) {
        return false;
    }
    is_delegation_only_block(&f.block)
}

#[derive(Debug, Clone)]
pub struct RustCodeDefinition {
    pub name: String,
    pub kind: CodeUnitKind,
    pub file: PathBuf,
    pub line: usize,
    pub impl_for_type: Option<String>,
}

pub(super) fn collect_rust_definitions(
    ast: &syn::File,
    file: &Path,
    defs: &mut Vec<RustCodeDefinition>,
) {
    for item in &ast.items {
        collect_definitions_from_item(item, file, defs);
    }
}

pub(crate) fn is_private(name: &str) -> bool {
    name.starts_with('_')
}

pub(super) fn try_add_def(
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

pub(super) fn extract_type_name(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(p) = ty {
        p.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

pub(super) fn collect_impl_methods(
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

pub(super) fn collect_definitions_from_item(
    item: &Item,
    file: &Path,
    defs: &mut Vec<RustCodeDefinition>,
) {
    match item {
        Item::Fn(f) if !has_test_attribute(&f.attrs) && !is_trivial_binary_main(f, file) => {
            try_add_def(
                defs,
                &f.sig.ident.to_string(),
                CodeUnitKind::Function,
                file,
                f.sig.ident.span().start().line,
                None,
            );
        }
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

pub(super) fn collect_test_module_references(ast: &syn::File, refs: &mut HashSet<String>) {
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

#[cfg(test)]
mod definitions_coverage {
    use super::*;

    #[test]
    fn well_known_constructors_recognized() {
        for name in ["Ok", "Err", "Some", "None"] {
            assert!(is_well_known_constructor(name));
        }
        assert!(!is_well_known_constructor("MyType"));
    }

    #[test]
    fn is_delegation_only_block_variants() {
        assert!(is_delegation_only_block(&syn::parse_str("{}").unwrap()));
        assert!(is_delegation_only_block(
            &syn::parse_str("{ crate::run() }").unwrap()
        ));
        assert!(!is_delegation_only_block(
            &syn::parse_str("{ struct Foo; }").unwrap()
        ));
    }

    #[test]
    fn is_trivial_expr_variants() {
        assert!(is_trivial_expr(&syn::parse_str("42").unwrap()));
        assert!(is_trivial_expr(&syn::parse_str("x").unwrap()));
        assert!(is_trivial_expr(&syn::parse_str("lib::run()").unwrap()));
        assert!(!is_trivial_expr(&syn::parse_str("|| {}").unwrap()));
    }

    #[test]
    fn is_trivial_stmt_variants() {
        assert!(is_trivial_stmt(
            &syn::parse_str::<syn::Stmt>("Ok(());").unwrap()
        ));
        let trivial: syn::Block = syn::parse_str("{ let x = 42; }").unwrap();
        assert!(trivial.stmts.iter().all(is_trivial_stmt));
        let non_trivial: syn::Block = syn::parse_str("{ fn inner() {} }").unwrap();
        assert!(!non_trivial.stmts.iter().all(is_trivial_stmt));
    }

    #[test]
    fn is_qualified_or_known_call_variants() {
        assert!(is_qualified_or_known_call(
            &syn::parse_str("module::func()").unwrap()
        ));
        assert!(is_qualified_or_known_call(
            &syn::parse_str("Ok(())").unwrap()
        ));
        assert!(!is_qualified_or_known_call(
            &syn::parse_str("unknown_func()").unwrap()
        ));
    }

    #[test]
    fn try_add_def_public_and_private() {
        let mut defs = Vec::new();
        try_add_def(
            &mut defs,
            "my_func",
            CodeUnitKind::Function,
            Path::new("t.rs"),
            1,
            None,
        );
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "my_func");
        try_add_def(
            &mut defs,
            "_private",
            CodeUnitKind::Function,
            Path::new("t.rs"),
            1,
            None,
        );
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn collect_rust_definitions_on_file() {
        let code = "fn public_fn() {}\nfn _private_fn() {}\nstruct MyStruct;";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut defs = Vec::new();
        collect_rust_definitions(&ast, Path::new("test.rs"), &mut defs);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"public_fn"));
        assert!(names.contains(&"MyStruct"));
        assert!(!names.contains(&"_private_fn"));
    }

    #[test]
    fn collect_test_module_references_finds_refs() {
        let code = r"
            fn production_fn() {}
            #[cfg(test)]
            mod tests {
                use super::*;
                #[test]
                fn test_it() { production_fn(); }
            }
        ";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut refs = HashSet::new();
        collect_test_module_references(&ast, &mut refs);
        assert!(refs.contains("production_fn"));
    }
}
