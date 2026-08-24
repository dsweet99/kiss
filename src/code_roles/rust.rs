use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::rust_include::canonical_path;
use crate::rust_parsing::ParsedRustFile;

use super::cfg_pred::{AtomInterner, CfgPred};
use super::error::RoleBuildError;
use super::facts::{FileRoleFacts, RoleRange};
use super::index::SourceRoleIndex;
use super::rust_cargo::{CargoRoot, cargo_roots_for_files};
use super::rust_include_parse::{IncludeAst, IncludeKind, parse_include_source};
use super::rust_walk::{WalkOutput, walk_file};
use super::span::SourceSpan;
use super::sweep::normalize_ranges;
use super::types::CodeContextSet;

struct WorkItem {
    path: PathBuf,
    pred: CfgPred,
    allow_production: bool,
    kind: IncludeKind,
}

pub fn classify_rust(
    parsed: &[&ParsedRustFile],
    discovered: &[PathBuf],
) -> Result<SourceRoleIndex, RoleBuildError> {
    let mut by_path: HashMap<PathBuf, &ParsedRustFile> = HashMap::new();
    for file in parsed {
        by_path.insert(canonical_path(&file.path), *file);
    }
    let paths: Vec<PathBuf> = discovered
        .iter()
        .chain(parsed.iter().map(|p| &p.path))
        .map(|p| canonical_path(p))
        .collect();
    let (cargo_roots, _) = cargo_roots_for_files(&paths)?;
    let mut atoms = AtomInterner::new();
    let mut acc: HashMap<PathBuf, Vec<RoleRange>> = HashMap::new();
    let mut base: HashMap<PathBuf, CodeContextSet> = HashMap::new();
    let mut queue = seed_queue(&cargo_roots, &paths);
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(item) = queue.pop_front() {
        let key = work_key(&item);
        if !seen.insert(key) {
            continue;
        }
        process_work_item(item, &by_path, &mut atoms, &mut acc, &mut base, &mut queue)?;
    }
    Ok(finish_rust_index(&paths, acc, base))
}

fn seed_queue(cargo_roots: &[CargoRoot], paths: &[PathBuf]) -> VecDeque<WorkItem> {
    let mut queue = VecDeque::new();
    if cargo_roots.is_empty() {
        for path in loose_seed_paths(paths) {
            queue.push_back(WorkItem {
                path: path.clone(),
                pred: CfgPred::True,
                allow_production: true,
                kind: IncludeKind::Items,
            });
        }
        return queue;
    }
    for root in cargo_roots {
        queue.push_back(WorkItem {
            path: root.src_path.clone(),
            pred: CfgPred::True,
            allow_production: root.allow_production,
            kind: IncludeKind::Items,
        });
    }
    queue
}

fn loose_seed_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let roots: Vec<PathBuf> = paths
        .iter()
        .filter(|path| is_loose_crate_root(path))
        .cloned()
        .collect();
    if roots.is_empty() {
        paths.to_vec()
    } else {
        roots
    }
}

fn is_loose_crate_root(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("lib.rs" | "main.rs")
    )
}

fn work_key(item: &WorkItem) -> String {
    format!(
        "{}:{}:{}",
        item.path.display(),
        item.allow_production,
        pred_key(&item.pred)
    )
}

fn pred_key(pred: &CfgPred) -> String {
    format!("{pred:?}")
}

fn process_work_item(
    item: WorkItem,
    by_path: &HashMap<PathBuf, &ParsedRustFile>,
    atoms: &mut AtomInterner,
    acc: &mut HashMap<PathBuf, Vec<RoleRange>>,
    base: &mut HashMap<PathBuf, CodeContextSet>,
    queue: &mut VecDeque<WorkItem>,
) -> Result<(), RoleBuildError> {
    let path = canonical_path(&item.path);
    let walked = walk_path(
        &path,
        item.kind,
        &item.pred,
        item.allow_production,
        by_path,
        atoms,
    )?;
    let ctx = super::cfg_sat::contexts_for_pred(&item.pred, item.allow_production);
    let entry = base
        .entry(path.clone())
        .or_insert_with(CodeContextSet::none);
    *entry = entry.union(ctx);
    acc.entry(path).or_default().extend(walked.ranges);
    enqueue_edges(walked.mods, walked.includes, item.allow_production, queue);
    Ok(())
}

fn walk_path(
    path: &Path,
    kind: IncludeKind,
    pred: &CfgPred,
    allow_production: bool,
    by_path: &HashMap<PathBuf, &ParsedRustFile>,
    atoms: &mut AtomInterner,
) -> Result<WalkOutput, RoleBuildError> {
    if let Some(parsed) = by_path.get(path) {
        return walk_file(path, &parsed.ast, pred, allow_production, atoms);
    }
    if !path.is_file() {
        return Err(missing_source(path, kind));
    }
    let source = std::fs::read_to_string(path).map_err(|_| missing_source(path, kind))?;
    walk_unparsed(path, &source, kind, pred, allow_production, atoms)
}

fn walk_unparsed(
    path: &Path,
    source: &str,
    kind: IncludeKind,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
) -> Result<WalkOutput, RoleBuildError> {
    match parse_include_source(path, source, kind)? {
        IncludeAst::File(file) => walk_file(path, &file, pred, allow_production, atoms),
        IncludeAst::Stmts(stmts) => {
            let mut out = WalkOutput {
                ranges: Vec::new(),
                mods: Vec::new(),
                includes: Vec::new(),
            };
            walk_stmt_list(path, &stmts, pred, allow_production, atoms, &mut out)?;
            Ok(out)
        }
        IncludeAst::Expr(_) | IncludeAst::Type(_) | IncludeAst::Pat(_) => Ok(WalkOutput {
            ranges: vec![RoleRange {
                span: SourceSpan::whole_file(source),
                contexts: super::cfg_sat::contexts_for_pred(pred, allow_production),
            }],
            mods: Vec::new(),
            includes: Vec::new(),
        }),
    }
}

fn walk_stmt_list(
    path: &Path,
    stmts: &[syn::Stmt],
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    super::rust_walk::walk_stmts(path, stmts, pred, allow_production, atoms, out)
}

fn enqueue_edges(
    mods: Vec<super::rust_modules::ModEdge>,
    includes: Vec<(PathBuf, CfgPred, IncludeKind)>,
    allow_production: bool,
    queue: &mut VecDeque<WorkItem>,
) {
    for edge in mods {
        queue.push_back(WorkItem {
            path: edge.target,
            pred: edge.pred,
            allow_production,
            kind: IncludeKind::Items,
        });
    }
    for (target, pred, kind) in includes {
        queue.push_back(WorkItem {
            path: target,
            pred,
            allow_production,
            kind,
        });
    }
}

fn finish_rust_index(
    paths: &[PathBuf],
    acc: HashMap<PathBuf, Vec<RoleRange>>,
    mut base: HashMap<PathBuf, CodeContextSet>,
) -> SourceRoleIndex {
    let mut index = SourceRoleIndex::empty();
    let mut seen = HashSet::new();
    for path in paths {
        let key = canonical_path(path);
        if !seen.insert(key.clone()) {
            continue;
        }
        let ctx = base
            .remove(&key)
            .unwrap_or_else(CodeContextSet::production_only);
        let ranges = acc.get(&key).cloned().unwrap_or_default();
        let ranges = normalize_ranges(ranges);
        index.insert(key, FileRoleFacts::new(ctx, ranges));
    }
    for (path, ranges) in acc {
        if seen.contains(&path) {
            continue;
        }
        let ctx = base
            .remove(&path)
            .unwrap_or_else(CodeContextSet::production_only);
        index.insert(path, FileRoleFacts::new(ctx, normalize_ranges(ranges)));
    }
    index
}

fn missing_source(path: &Path, kind: IncludeKind) -> RoleBuildError {
    if kind == IncludeKind::Items {
        RoleBuildError::MissingModule {
            from: PathBuf::from("<root>"),
            name: path.display().to_string(),
        }
    } else {
        RoleBuildError::MissingInclude {
            from: PathBuf::from("<root>"),
            target: path.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod rust_roles_test {
    use super::*;
    use crate::code_roles::types::CodeRole;
    use crate::rust_parsing::parse_rust_file;

    #[test]
    fn cfg_test_mod_is_test_only_in_loose_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lib.rs");
        std::fs::write(
            &path,
            "pub fn prod() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n",
        )
        .unwrap();
        let parsed = parse_rust_file(&path).unwrap();
        let index = classify_rust(&[&parsed], std::slice::from_ref(&path)).unwrap();
        assert_eq!(index.role_at(&path, 1), CodeRole::Production);
        let test_line = parsed
            .source
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("fn t"))
            .map(|(i, _)| i + 1)
            .unwrap();
        assert_eq!(index.role_at(&path, test_line), CodeRole::TestOnly);
        assert_eq!(
            index.file_composition(&path),
            super::super::types::FileComposition::Mixed
        );
    }

    #[test]
    fn cfg_test_field_is_test_only() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lib.rs");
        std::fs::write(
            &path,
            "pub struct S {\n    pub a: i32,\n    #[cfg(test)]\n    pub t: i32,\n}\n",
        )
        .unwrap();
        let parsed = parse_rust_file(&path).unwrap();
        let index = classify_rust(&[&parsed], std::slice::from_ref(&path)).unwrap();
        let field = parsed.ast.items.iter().find_map(|item| match item {
            syn::Item::Struct(st) => st
                .fields
                .iter()
                .find(|f| f.ident.as_ref().is_some_and(|id| id == "t")),
            _ => None,
        });
        let span = crate::code_roles::SourceSpan::of_syn(field.unwrap());
        assert_eq!(index.role_for_span(&path, span), CodeRole::TestOnly);
        assert_eq!(index.role_at(&path, 2), CodeRole::Production);
    }

    #[test]
    fn test_fn_without_cfg_is_test_only() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lib.rs");
        std::fs::write(&path, "#[test]\nfn t() {}\npub fn p() {}\n").unwrap();
        let parsed = parse_rust_file(&path).unwrap();
        let index = classify_rust(&[&parsed], std::slice::from_ref(&path)).unwrap();
        let ast_fn = parsed
            .ast
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Fn(f) if f.sig.ident == "t" => {
                    Some(crate::code_roles::SourceSpan::of_syn(f))
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(index.role_for_span(&path, ast_fn), CodeRole::TestOnly);
        assert_eq!(index.role_at(&path, 3), CodeRole::Production);
    }

    #[test]
    fn cfg_test_external_mod_file_is_test_only() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib.rs");
        let helper = tmp.path().join("helper.rs");
        std::fs::write(&lib, "pub fn prod() {}\n#[cfg(test)]\nmod helper;\n").unwrap();
        std::fs::write(&helper, "pub fn support() {}\n").unwrap();
        let parsed_lib = parse_rust_file(&lib).unwrap();
        let parsed_helper = parse_rust_file(&helper).unwrap();
        let files = [lib.clone(), helper.clone()];
        let index = classify_rust(&[&parsed_lib, &parsed_helper], &files).unwrap();
        assert_eq!(
            index.file_composition(&helper),
            super::super::types::FileComposition::TestOnly,
            "helper.rs reached only via #[cfg(test)] mod helper must be test-only"
        );
        assert_ne!(
            index.file_composition(&lib),
            super::super::types::FileComposition::TestOnly
        );
    }

    #[test]
    fn three_level_cfg_test_mod_chain_is_test_only() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib.rs");
        let tests = tmp.path().join("tests.rs");
        let helpers_dir = tmp.path().join("tests");
        std::fs::create_dir_all(helpers_dir.join("helpers")).unwrap();
        let helpers = helpers_dir.join("helpers.rs");
        let inner = helpers_dir.join("helpers").join("inner.rs");
        std::fs::write(&lib, "#[cfg(test)]\nmod tests;\n").unwrap();
        std::fs::write(&tests, "mod helpers;\n").unwrap();
        std::fs::write(&helpers, "mod inner;\n").unwrap();
        std::fs::write(&inner, "pub fn h() {}\n").unwrap();
        let parsed = [
            parse_rust_file(&lib).unwrap(),
            parse_rust_file(&tests).unwrap(),
            parse_rust_file(&helpers).unwrap(),
            parse_rust_file(&inner).unwrap(),
        ];
        let files = [lib, tests, helpers, inner.clone()];
        let refs: Vec<_> = parsed.iter().collect();
        let index = classify_rust(&refs, &files).unwrap();
        assert_eq!(
            index.file_composition(&inner),
            super::super::types::FileComposition::TestOnly
        );
    }
}
