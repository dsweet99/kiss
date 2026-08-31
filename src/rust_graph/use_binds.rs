use syn::UseTree;
use syn::visit::Visit;

pub(crate) fn collect_file_use_binds(ast: &syn::File) -> Vec<(String, String, usize)> {
    let mut visitor = UseBindVisitor { out: Vec::new() };
    visitor.visit_file(ast);
    visitor.out
}

struct UseBindVisitor {
    out: Vec<(String, String, usize)>,
}

impl<'ast> Visit<'ast> for UseBindVisitor {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let mut prefix = Vec::new();
        let line = syn::spanned::Spanned::span(&node.tree).start().line;
        collect_use_binds(&node.tree, &mut prefix, line, &mut self.out);
        syn::visit::visit_item_use(self, node);
    }
}

fn collect_use_binds(
    tree: &UseTree,
    prefix: &mut Vec<String>,
    line: usize,
    out: &mut Vec<(String, String, usize)>,
) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_use_binds(&path.tree, prefix, line, out);
            prefix.pop();
        }
        UseTree::Name(name) => push_bind(prefix, name.ident.to_string(), line, out),
        UseTree::Rename(rename) => push_bind(prefix, rename.ident.to_string(), line, out),
        UseTree::Glob(_) => {}
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_binds(item, prefix, line, out);
            }
        }
    }
}

fn push_bind(prefix: &[String], last: String, line: usize, out: &mut Vec<(String, String, usize)>) {
    let module_prefix = prefix
        .iter()
        .filter(|seg| !matches!(seg.as_str(), "self" | "super" | "crate"))
        .cloned()
        .collect::<Vec<_>>()
        .join(".");
    if module_prefix.is_empty() {
        return;
    }
    out.push((module_prefix, last, line));
}

#[cfg(test)]
mod use_binds_test {
    use super::collect_file_use_binds;

    fn binds(src: &str) -> Vec<(String, String)> {
        collect_file_use_binds(&syn::parse_file(src).unwrap())
            .into_iter()
            .map(|(prefix, last, _line)| (prefix, last))
            .collect()
    }

    #[test]
    fn crate_path_names_last_ident() {
        let got = binds("use crate::m::Helper;");
        assert_eq!(got, vec![("m".to_string(), "Helper".to_string())]);
    }

    #[test]
    fn glob_does_not_name_nested() {
        assert!(binds("use crate::m::*;").is_empty());
    }

    #[test]
    fn group_and_rename_keep_original_ident() {
        let got = binds("use crate::m::{Helper as H, Other};");
        assert!(got.contains(&("m".into(), "Helper".into())));
        assert!(got.contains(&("m".into(), "Other".into())));
    }

    #[test]
    fn empty_prefix_is_not_a_nested_bind() {
        assert!(binds("use Helper;").is_empty());
    }
}
