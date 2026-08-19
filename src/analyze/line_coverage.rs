use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use kiss::check_universe_cache::CachedLineCoverageRecord;
use rayon::prelude::*;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::analyze::coverage_gate::is_coverage_gate_file;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeCoverageSnapshot {
    pub(crate) identity: String,
    pub(crate) covered_lines: BTreeMap<String, BTreeSet<u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LineCoverageRecord {
    pub(crate) file: PathBuf,
    pub(crate) total_lines: usize,
    pub(crate) covered_lines: usize,
    pub(crate) percent: usize,
    pub(crate) first_uncovered_line: Option<usize>,
}

pub(crate) fn compute_line_coverage_records(
    repo_root: &Path,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    snapshot: &RuntimeCoverageSnapshot,
) -> Vec<LineCoverageRecord> {
    let cfg_test_only_rust_files = cfg_test_only_rust_files(rs_files);
    let paths: Vec<&PathBuf> = py_files
        .iter()
        .chain(rs_files)
        .filter(|path| is_coverage_gate_file(path))
        .filter(|path| !cfg_test_only_rust_files.contains(*path))
        .collect();
    let mut records = paths
        .into_par_iter()
        .map(|path| compute_file_line_coverage(repo_root, path, snapshot))
        .collect::<Vec<_>>();
    records.sort_by(|a, b| a.file.cmp(&b.file));
    records
}

#[path = "line_coverage_cfg.rs"]
mod line_coverage_cfg;
use line_coverage_cfg::{cfg_attrs_active, coverage_off_attrs, stmt_cfg_active};

fn cfg_test_only_rust_files(rs_files: &[PathBuf]) -> BTreeSet<PathBuf> {
    let universe: BTreeSet<PathBuf> = rs_files.iter().cloned().collect();
    let mut refs: HashMap<PathBuf, Vec<(PathBuf, bool)>> = HashMap::new();
    for file in rs_files {
        let Ok(source) = fs::read_to_string(file) else {
            continue;
        };
        let Ok(ast) = syn::parse_file(&source) else {
            continue;
        };
        for item in ast.items {
            let syn::Item::Mod(module) = item else {
                continue;
            };

            if module.content.is_none() {
                let Some(target) = resolve_module_file(file, &module) else {
                    continue;
                };
                if universe.contains(&target) {
                    refs.entry(target)
                        .or_default()
                        .push((file.clone(), has_cfg_test_attribute(&module.attrs)));
                }
            }
        }
    }
    let mut test_only = BTreeSet::new();
    loop {
        let before = test_only.len();
        for (path, incoming) in &refs {
            if !incoming.is_empty()
                && incoming.iter().all(|(parent, edge_is_test_only)| {
                    *edge_is_test_only || test_only.contains(parent)
                })
            {
                test_only.insert(path.clone());
            }
        }
        if test_only.len() == before {
            return test_only;
        }
    }
}

fn resolve_module_file(parent_file: &Path, module: &syn::ItemMod) -> Option<PathBuf> {
    if let Some(path_attr) = module_path_attr(module) {
        return Some(parent_file.parent()?.join(path_attr));
    }
    let name = module.ident.to_string();
    let sibling = parent_file.parent()?.join(format!("{name}.rs"));
    if sibling.exists() {
        return Some(sibling);
    }
    Some(parent_file.parent()?.join(name).join("mod.rs"))
}

fn module_path_attr(module: &syn::ItemMod) -> Option<PathBuf> {
    module.attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("path") {
            return None;
        }
        let syn::Meta::NameValue(name_value) = &attr.meta else {
            return None;
        };
        let syn::Expr::Lit(expr_lit) = &name_value.value else {
            return None;
        };
        let syn::Lit::Str(lit) = &expr_lit.lit else {
            return None;
        };
        Some(PathBuf::from(lit.value()))
    })
}

fn has_cfg_test_attribute(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let syn::Meta::List(list) = &attr.meta else {
            return false;
        };
        cfg_tokens_contain_test(list.tokens.clone())
    })
}

fn cfg_tokens_contain_test(tokens: proc_macro2::TokenStream) -> bool {
    let mut iter = tokens.into_iter();
    while let Some(token) = iter.next() {
        match token {
            proc_macro2::TokenTree::Ident(ident) if ident == "test" => return true,
            proc_macro2::TokenTree::Ident(ident) if ident == "not" => {
                let _ = iter.next();
            }
            proc_macro2::TokenTree::Ident(ident) if ident == "all" => {
                if let Some(proc_macro2::TokenTree::Group(group)) = iter.next()
                    && cfg_tokens_contain_test(group.stream())
                {
                    return true;
                }
            }
            proc_macro2::TokenTree::Ident(ident) if ident == "any" => {
                if let Some(proc_macro2::TokenTree::Group(group)) = iter.next()
                    && cfg_tokens_contain_test(group.stream())
                {
                    return true;
                }
            }
            proc_macro2::TokenTree::Group(group) => {
                if cfg_tokens_contain_test(group.stream()) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn compute_file_line_coverage(
    repo_root: &Path,
    file: &Path,
    snapshot: &RuntimeCoverageSnapshot,
) -> LineCoverageRecord {
    let Some(denominator_lines) = coverage_denominator_lines(file) else {

        return LineCoverageRecord {
            file: file.to_path_buf(),
            total_lines: 0,
            covered_lines: 0,
            percent: 0,
            first_uncovered_line: None,
        };
    };
    let total_lines = denominator_lines.len();
    let rel = repo_relative_key(repo_root, file);
    let covered = rel
        .as_ref()
        .and_then(|key| snapshot.covered_lines.get(key))
        .map_or(0, |lines| {
            lines
                .iter()
                .filter(|line| denominator_lines.contains(&(**line as usize)))
                .collect::<BTreeSet<_>>()
                .len()
        });
    let first_uncovered_line = denominator_lines
        .iter()
        .find(|line| {
            rel.as_ref()
                .and_then(|key| snapshot.covered_lines.get(key))
                .is_none_or(|lines| !lines.contains(&(**line as u32)))
        })
        .copied();
    let percent = if total_lines == 0 {
        100
    } else {
        coverage_percentage(covered, total_lines)
    };
    LineCoverageRecord {
        file: file.to_path_buf(),
        total_lines,
        covered_lines: covered,
        percent,
        first_uncovered_line,
    }
}

pub(crate) fn cached_line_records(records: &[LineCoverageRecord]) -> Vec<CachedLineCoverageRecord> {
    records
        .iter()
        .map(|record| CachedLineCoverageRecord {
            file: record.file.to_string_lossy().to_string(),
            total_lines: record.total_lines,
            covered_lines: record.covered_lines,
            percent: record.percent,
            first_uncovered_line: record.first_uncovered_line,
        })
        .collect()
}

pub(crate) fn line_records_from_cache(
    records: &[CachedLineCoverageRecord],
) -> Vec<LineCoverageRecord> {
    records
        .iter()
        .map(|record| LineCoverageRecord {
            file: PathBuf::from(&record.file),
            total_lines: record.total_lines,
            covered_lines: record.covered_lines,
            percent: record.percent,
            first_uncovered_line: record.first_uncovered_line,
        })
        .collect()
}

/// Returns `None` when the source file cannot be read (fail closed for coverage %).
/// Returns `Some(empty)` for a readable empty file (treated as 100% covered).
fn coverage_denominator_lines(file: &Path) -> Option<BTreeSet<usize>> {
    let contents = fs::read_to_string(file).ok()?;
    if contents.is_empty() {
        return Some(BTreeSet::new());
    }
    let parsed = match file.extension().and_then(|ext| ext.to_str()) {
        Some("py") => python_coverable_lines(&contents),
        Some("rs") => rust_coverable_lines(&contents),
        _ => None,
    };
    Some(parsed.unwrap_or_else(|| (1..=contents.lines().count()).collect()))
}

fn python_coverable_lines(source: &str) -> Option<BTreeSet<usize>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(source, None)?;
    let mut lines = BTreeSet::new();
    collect_python_coverable_lines(tree.root_node(), &mut lines);
    Some(lines)
}

fn collect_python_coverable_lines(node: tree_sitter::Node<'_>, lines: &mut BTreeSet<usize>) {
    if is_python_coverable_node(node.kind()) {
        lines.insert(node.start_position().row + 1);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_python_coverable_lines(child, lines);
    }
}

fn is_python_coverable_node(kind: &str) -> bool {
    matches!(
        kind,
        "assert_statement"
            | "assignment"
            | "augmented_assignment"
            | "class_definition"
            | "decorated_definition"
            | "delete_statement"
            | "expression_statement"
            | "for_statement"
            | "function_definition"
            | "future_import_statement"
            | "global_statement"
            | "if_statement"
            | "import_from_statement"
            | "import_statement"
            | "nonlocal_statement"
            | "pass_statement"
            | "raise_statement"
            | "return_statement"
            | "try_statement"
            | "while_statement"
            | "with_statement"
            | "yield"
    )
}

fn rust_coverable_lines(source: &str) -> Option<BTreeSet<usize>> {
    let ast = syn::parse_file(source).ok()?;
    let mut visitor = RustCoverableLineVisitor {
        source_lines: source.lines().collect(),
        lines: BTreeSet::new(),
    };
    visitor.visit_file(&ast);
    Some(visitor.lines)
}

struct RustCoverableLineVisitor<'a> {
    source_lines: Vec<&'a str>,
    lines: BTreeSet<usize>,
}

impl RustCoverableLineVisitor<'_> {
    fn add_start_line(&mut self, span: proc_macro2::Span) {
        let line_no = span.start().line;
        let Some(line) = self.source_lines.get(line_no.saturating_sub(1)) else {
            return;
        };
        let trimmed = line.trim();


        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("#[")
            || trimmed == "else {"
            || trimmed == "} else {"
            || trimmed == "unsafe {"
        {
            return;
        }
        self.lines.insert(line_no);
    }
}

impl<'ast> Visit<'ast> for RustCoverableLineVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if !cfg_attrs_active(&node.attrs) || coverage_off_attrs(&node.attrs) {
            return;
        }
        self.add_start_line(node.sig.span());
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if !cfg_attrs_active(&node.attrs) || coverage_off_attrs(&node.attrs) {
            return;
        }
        self.add_start_line(node.sig.span());
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_expr_block(&mut self, node: &'ast syn::ExprBlock) {
        if !cfg_attrs_active(&node.attrs) {
            return;
        }
        syn::visit::visit_expr_block(self, node);
    }

    fn visit_stmt(&mut self, node: &'ast syn::Stmt) {
        if !stmt_cfg_active(node) {
            return;
        }
        self.add_start_line(node.span());
        syn::visit::visit_stmt(self, node);
    }
}

fn repo_relative_key(repo_root: &Path, file: &Path) -> Option<String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    canonical
        .strip_prefix(root)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

pub(crate) fn coverage_percentage(covered: usize, total: usize) -> usize {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    {
        ((covered as f64 / total as f64) * 100.0).round() as usize
    }
}

#[cfg(test)]
#[path = "line_coverage_tests/mod.rs"]
mod tests;
