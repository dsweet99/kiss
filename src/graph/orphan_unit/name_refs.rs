use crate::graph::orphan_unit::extract::NamedBind;
use crate::graph::{
    ContextDependencyGraph, module_name_for_path, qualified_module_name, resolve_import,
};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use syn::ItemImpl;
use syn::visit::Visit;

pub(super) fn collect_name_binds(
    py: &[ParsedFile],
    rs: &[ParsedRustFile],
    py_ctx: &ContextDependencyGraph,
    rs_ctx: &ContextDependencyGraph,
) -> Vec<NamedBind> {
    let mut out = Vec::new();
    out.extend(python_name_binds(py, py_ctx));
    out.extend(rust_name_binds(rs, rs_ctx));
    out
}

fn python_name_binds(py: &[ParsedFile], ctx: &ContextDependencyGraph) -> Vec<NamedBind> {
    let prod = ctx.production_view();
    let mut bare: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for name in prod.nodes.keys() {
        let bare_name = name.rsplit('.').next().unwrap_or(name).to_string();
        bare.entry(bare_name).or_default().push(name.clone());
    }
    let mut out = Vec::new();
    for parsed in py {
        let importer = module_name_for_path(ctx, &parsed.path)
            .unwrap_or_else(|| qualified_module_name(&parsed.path));
        let parent = importer.rsplit_once('.').map(|(p, _)| p);
        for (prefix, last, line) in python_ident_refs(parsed) {
            if prefix.is_empty() {
                out.push(NamedBind {
                    file: parsed.path.clone(),
                    line,
                    target_module: String::new(),
                    last,
                });
                continue;
            }
            for resolved in resolve_import(&prefix, parent, &bare) {
                out.push(NamedBind {
                    file: parsed.path.clone(),
                    line,
                    target_module: resolved,
                    last: last.clone(),
                });
            }
            if prod.nodes.contains_key(&prefix) {
                out.push(NamedBind {
                    file: parsed.path.clone(),
                    line,
                    target_module: prefix,
                    last,
                });
            }
        }
    }
    out
}

fn rust_name_binds(rs: &[ParsedRustFile], ctx: &ContextDependencyGraph) -> Vec<NamedBind> {
    let prod = ctx.production_view();
    let last_idx = crate::graph::orphan_unit::extract::rust_last_index(&prod);
    let mut out = Vec::new();
    for parsed in rs {
        let module = module_name_for_path(ctx, &parsed.path);
        for (prefix, last, line) in collect_file_name_refs(&parsed.ast) {
            out.push(NamedBind {
                file: parsed.path.clone(),
                line,
                target_module: String::new(),
                last: last.clone(),
            });
            if prefix.is_empty() {
                continue;
            }
            if let Some(resolved) = crate::graph::orphan_unit::extract::resolve_rust_prefix_from(
                &prefix,
                module.as_deref(),
                &prod,
                &last_idx,
            ) {
                out.push(NamedBind {
                    file: parsed.path.clone(),
                    line,
                    target_module: resolved,
                    last,
                });
            }
        }
    }
    out
}

fn collect_file_name_refs(ast: &syn::File) -> Vec<(String, String, usize)> {
    let mut visitor = NameRefVisitor {
        out: Vec::new(),
        skip_path: false,
    };
    visitor.visit_file(ast);
    visitor.out
}

struct NameRefVisitor {
    out: Vec<(String, String, usize)>,
    skip_path: bool,
}

impl<'ast> Visit<'ast> for NameRefVisitor {
    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let skip_self = node.trait_.is_none();
        self.skip_path = skip_self;
        self.visit_type(&node.self_ty);
        self.skip_path = false;
        if let Some((_, path, _)) = &node.trait_ {
            self.visit_path(path);
        }
        for item in &node.items {
            self.visit_impl_item(item);
        }
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if !self.skip_path {
            push_path(path, &mut self.out);
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let line = syn::spanned::Spanned::span(&node.method).start().line;
        self.out
            .push((String::new(), node.method.to_string(), line));
        syn::visit::visit_expr_method_call(self, node);
    }
}

fn push_path(path: &syn::Path, out: &mut Vec<(String, String, usize)>) {
    let segs: Vec<String> = path
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .filter(|seg| !matches!(seg.as_str(), "self" | "super" | "crate"))
        .collect();
    let Some(last) = segs.last().cloned() else {
        return;
    };
    let line = syn::spanned::Spanned::span(path).start().line;
    for seg in &segs {
        out.push((String::new(), seg.clone(), line));
    }
    let prefix = segs[..segs.len().saturating_sub(1)].join(".");
    out.push((prefix, last, line));
}

fn python_ident_refs(parsed: &ParsedFile) -> Vec<(String, String, usize)> {
    let mut out = Vec::new();
    walk_py(parsed.tree.root_node(), parsed.source.as_bytes(), &mut out);
    out
}

fn walk_py(node: tree_sitter::Node<'_>, src: &[u8], out: &mut Vec<(String, String, usize)>) {
    match node.kind() {
        "function_definition" | "class_definition" => {
            walk_py_skip_name(node, src, out);
            return;
        }
        "attribute" => {
            if let (Some(obj), Some(attr)) = (
                node.child_by_field_name("object"),
                node.child_by_field_name("attribute"),
            ) && let (Ok(prefix), Ok(last)) = (obj.utf8_text(src), attr.utf8_text(src))
            {
                out.push((
                    prefix.to_string(),
                    last.to_string(),
                    node.start_position().row + 1,
                ));
            }
        }
        "identifier" => {
            if let Ok(name) = node.utf8_text(src) {
                out.push((
                    String::new(),
                    name.to_string(),
                    node.start_position().row + 1,
                ));
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_py(child, src, out);
    }
}

fn walk_py_skip_name(
    node: tree_sitter::Node<'_>,
    src: &[u8],
    out: &mut Vec<(String, String, usize)>,
) {
    let name = node.child_by_field_name("name");
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if name.is_some_and(|n| n.id() == child.id()) {
            continue;
        }
        walk_py(child, src, out);
    }
}

#[cfg(test)]
mod name_refs_test {
    use super::collect_file_name_refs;

    fn refs(src: &str) -> Vec<(String, String)> {
        collect_file_name_refs(&syn::parse_file(src).unwrap())
            .into_iter()
            .map(|(prefix, last, _line)| (prefix, last))
            .collect()
    }

    #[test]
    fn path_expr_names_last_ident() {
        let got = refs("fn f() { let _ = crate::m::Helper; }");
        assert!(got.contains(&("m".into(), "Helper".into())));
    }

    #[test]
    fn inherent_impl_self_ty_is_not_a_ref() {
        let got = refs("struct Helper;\nimpl Helper { fn f() {} }");
        assert!(!got.iter().any(|(p, last)| p.is_empty() && last == "Helper"));
    }

    #[test]
    fn enum_variant_path_names_the_type() {
        let got = refs("fn f() { let _ = Helper::A; }");
        assert!(got.iter().any(|(p, last)| p.is_empty() && last == "Helper"));
    }

    #[test]
    fn method_call_names_the_method() {
        let got = refs("fn f(x: i32) { let _ = x.abs(); }");
        assert!(got.iter().any(|(_, last)| last == "abs"));
    }
}
