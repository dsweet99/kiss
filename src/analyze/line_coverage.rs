use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use kiss::check_universe_cache::CachedLineCoverageRecord;
use kiss::code_roles::{SourceRoleIndex, skip_syn};
use kiss::{ParsedFile, ParsedRustFile};
use syn::spanned::Spanned;
use syn::visit::Visit;

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

#[derive(Clone, Debug)]
pub(crate) struct CoverageSourceFacts {
    pub(crate) roles: SourceRoleIndex,
    coverable_lines: BTreeMap<PathBuf, BTreeSet<usize>>,
}

impl CoverageSourceFacts {
    pub(crate) fn from_files(
        py_files: &[PathBuf],
        rs_files: &[PathBuf],
    ) -> Result<Self, kiss::code_roles::RoleBuildError> {
        let (py, rs, roles) = crate::analyze_parse::parse_classified(py_files, rs_files)?;
        Ok(Self::from_index(roles, &py, &rs, py_files, rs_files))
    }

    pub(crate) fn from_index(
        roles: SourceRoleIndex,
        py: &[ParsedFile],
        rs: &[ParsedRustFile],
        py_files: &[PathBuf],
        rs_files: &[PathBuf],
    ) -> Self {
        let rs_by = parsed_by_path(rs, |parsed| parsed.path.as_path());
        let py_by = parsed_by_path(py, |parsed| parsed.path.as_path());
        let paths: Vec<&PathBuf> = py_files
            .iter()
            .chain(rs_files)
            .filter(|path| {
                roles.file_composition(path) != kiss::code_roles::FileComposition::TestOnly
            })
            .collect();
        let coverable_lines: BTreeMap<_, _> = paths
            .into_iter()
            .filter_map(|path| {
                let lines = coverable_lines_for_path(path, &roles, &rs_by, &py_by)?;
                Some((path.clone(), lines))
            })
            .collect();
        Self {
            roles,
            coverable_lines,
        }
    }

    pub(crate) fn coverable_map(&self) -> &BTreeMap<PathBuf, BTreeSet<usize>> {
        &self.coverable_lines
    }

    pub(crate) fn production_denoms(&self) -> Vec<CoverableDenom> {
        self.coverable_lines
            .iter()
            .map(|(path, denom)| {
                let candidates: Vec<usize> = denom.iter().copied().collect();
                let mut lines = self.roles.production_lines(path, &candidates);
                lines.sort_unstable();
                lines.dedup();
                CoverableDenom {
                    file: path.clone(),
                    lines,
                    mixed: self.roles.file_composition(path)
                        == kiss::code_roles::FileComposition::Mixed,
                }
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CoverableDenom {
    pub file: PathBuf,
    pub lines: Vec<usize>,
    pub mixed: bool,
}

fn parsed_by_path<T>(items: &[T], path_of: impl Fn(&T) -> &Path) -> HashMap<PathBuf, &T> {
    let mut map = HashMap::new();
    for item in items {
        let path = path_of(item);
        map.insert(path.to_path_buf(), item);
        map.insert(kiss::rust_include::canonical_path(path), item);
    }
    map
}

fn coverable_lines_for_path(
    path: &Path,
    roles: &SourceRoleIndex,
    rs_by: &HashMap<PathBuf, &ParsedRustFile>,
    py_by: &HashMap<PathBuf, &ParsedFile>,
) -> Option<BTreeSet<usize>> {
    let canon = kiss::rust_include::canonical_path(path);
    if let Some(parsed) = rs_by.get(path).or_else(|| rs_by.get(&canon)) {
        return rust_coverable_lines(&parsed.path, &parsed.source, roles, Some(&parsed.ast));
    }
    if let Some(parsed) = py_by.get(path).or_else(|| py_by.get(&canon)) {
        return Some(python_coverable_from_tree(&parsed.tree));
    }
    coverage_denominator_lines(path, roles)
}

#[cfg(test)]
pub(crate) fn compute_line_coverage_records(
    repo_root: &Path,
    facts: &CoverageSourceFacts,
    snapshot: &RuntimeCoverageSnapshot,
) -> Vec<LineCoverageRecord> {
    records_from_denoms(repo_root, &facts.production_denoms(), snapshot)
}

pub(crate) fn records_from_denoms(
    repo_root: &Path,
    denoms: &[CoverableDenom],
    snapshot: &RuntimeCoverageSnapshot,
) -> Vec<LineCoverageRecord> {
    let mut records: Vec<_> = denoms
        .iter()
        .map(|denom| record_from_prefiltered(repo_root, denom, snapshot))
        .collect();
    records.sort_by(|a, b| a.file.cmp(&b.file));
    records
}

#[path = "line_coverage_cfg.rs"]
mod line_coverage_cfg;
use line_coverage_cfg::coverage_off_attrs;

#[cfg(test)]
pub(crate) fn compute_file_line_coverage(
    repo_root: &Path,
    file: &Path,
    snapshot: &RuntimeCoverageSnapshot,
) -> LineCoverageRecord {
    compute_file_line_coverage_with_roles(
        repo_root,
        file,
        snapshot,
        &kiss::code_roles::SourceRoleIndex::empty(),
    )
}

#[cfg(test)]
fn compute_file_line_coverage_with_roles(
    repo_root: &Path,
    file: &Path,
    snapshot: &RuntimeCoverageSnapshot,
    roles: &SourceRoleIndex,
) -> LineCoverageRecord {
    let Some(denominator_lines) = coverage_denominator_lines(file, roles) else {
        return LineCoverageRecord {
            file: file.to_path_buf(),
            total_lines: 0,
            covered_lines: 0,
            percent: 0,
            first_uncovered_line: None,
        };
    };
    record_from_denominator(repo_root, file, &denominator_lines, snapshot, roles)
}

#[cfg(test)]
fn record_from_denominator(
    repo_root: &Path,
    file: &Path,
    denominator_lines: &BTreeSet<usize>,
    snapshot: &RuntimeCoverageSnapshot,
    roles: &SourceRoleIndex,
) -> LineCoverageRecord {
    let candidates: Vec<usize> = denominator_lines.iter().copied().collect();
    let mut lines = roles.production_lines(file, &candidates);
    lines.sort_unstable();
    lines.dedup();
    record_from_prefiltered(
        repo_root,
        &CoverableDenom {
            file: file.to_path_buf(),
            lines,
            mixed: roles.file_composition(file) == kiss::code_roles::FileComposition::Mixed,
        },
        snapshot,
    )
}

fn record_from_prefiltered(
    repo_root: &Path,
    denom: &CoverableDenom,
    snapshot: &RuntimeCoverageSnapshot,
) -> LineCoverageRecord {
    let denominator_lines: BTreeSet<usize> = denom.lines.iter().copied().collect();
    let total_lines = denominator_lines.len();
    if total_lines == 0 && denom.mixed {
        return LineCoverageRecord {
            file: denom.file.clone(),
            total_lines: 0,
            covered_lines: 0,
            percent: 100,
            first_uncovered_line: None,
        };
    }
    let rel = repo_relative_key(repo_root, &denom.file);
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
        file: denom.file.clone(),
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

fn coverage_denominator_lines(file: &Path, roles: &SourceRoleIndex) -> Option<BTreeSet<usize>> {
    let contents = fs::read_to_string(file).ok()?;
    if contents.is_empty() {
        return Some(BTreeSet::new());
    }
    let parsed = match file.extension().and_then(|ext| ext.to_str()) {
        Some("py") => python_coverable_lines(&contents),
        Some("rs") => rust_coverable_lines(file, &contents, roles, None),
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
    Some(python_coverable_from_tree(&tree))
}

fn python_coverable_from_tree(tree: &tree_sitter::Tree) -> BTreeSet<usize> {
    let mut lines = BTreeSet::new();
    collect_python_coverable_lines(tree.root_node(), &mut lines);
    lines
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
            | "match_statement"
            | "case_clause"
            | "break_statement"
            | "continue_statement"
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

fn rust_coverable_lines(
    path: &Path,
    source: &str,
    roles: &SourceRoleIndex,
    ast: Option<&syn::File>,
) -> Option<BTreeSet<usize>> {
    let parsed_ast;
    let ast = if let Some(ast) = ast {
        ast
    } else {
        parsed_ast = syn::parse_file(source).ok()?;
        &parsed_ast
    };
    let mut visitor = RustCoverableLineVisitor {
        source_lines: source.lines().collect(),
        lines: BTreeSet::new(),
        path,
        roles,
    };
    visitor.visit_file(ast);
    Some(visitor.lines)
}

struct RustCoverableLineVisitor<'a> {
    source_lines: Vec<&'a str>,
    lines: BTreeSet<usize>,
    path: &'a Path,
    roles: &'a SourceRoleIndex,
}

impl RustCoverableLineVisitor<'_> {
    fn skip_node(&self, node: &impl Spanned) -> bool {
        skip_syn(Some(self.roles), self.path, node)
    }

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
        if self.skip_node(node) || coverage_off_attrs(&node.attrs) {
            return;
        }
        self.add_start_line(node.sig.span());
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if self.skip_node(node) || coverage_off_attrs(&node.attrs) {
            return;
        }
        self.add_start_line(node.sig.span());
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_expr_block(&mut self, node: &'ast syn::ExprBlock) {
        if self.skip_node(node) {
            return;
        }
        syn::visit::visit_expr_block(self, node);
    }

    fn visit_stmt(&mut self, node: &'ast syn::Stmt) {
        if self.skip_node(node) {
            return;
        }
        self.add_start_line(node.span());
        syn::visit::visit_stmt(self, node);
    }
}

pub(crate) fn repo_relative_key(repo_root: &Path, file: &Path) -> Option<String> {
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
fn coverage_denominator_lines_for_test(file: &Path) -> Option<BTreeSet<usize>> {
    coverage_denominator_lines(file, &SourceRoleIndex::empty())
}

#[cfg(test)]
fn compute_line_coverage_records_for_test(
    repo_root: &Path,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    snapshot: &RuntimeCoverageSnapshot,
) -> Result<Vec<LineCoverageRecord>, kiss::code_roles::RoleBuildError> {
    let facts = CoverageSourceFacts::from_files(py_files, rs_files)?;
    Ok(compute_line_coverage_records(repo_root, &facts, snapshot))
}

#[cfg(test)]
#[path = "line_coverage_tests/mod.rs"]
mod tests;
