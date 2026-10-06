use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::ChangedTestSelectors;
use crate::test_runner::language_keyed::LanguageKeyed;
use kiss::{
    ContextDependencyGraph, ParsedFile, ParsedRustFile, module_name_for_path, path_for_module_name,
};

use super::super::ChangedFileTests;

pub(super) fn expand_unresolved_test_helpers(
    repo_root: &Path,
    test_paths: &[PathBuf],
    ignore: &[String],
    enumerated: &ChangedFileTests,
    changed: &mut ChangedTestSelectors,
) -> Result<(), String> {
    let unresolved = LanguageKeyed {
        python: unresolved_python_helper(test_paths, enumerated),
        rust: unresolved_rust_helper(test_paths, enumerated),
    };
    let mut ids = LanguageKeyed::<Vec<String>>::default();
    for language in crate::test_runner::lang_registry::languages() {
        if *unresolved.get(language) {
            *ids.get_mut(language) =
                crate::test_runner::lang_registry::enumerate_workspace_selectors(
                    repo_root,
                    language,
                    ignore,
                    &[],
                )?;
        }
    }
    super::extend_tagged(changed, ids);
    Ok(())
}

fn unresolved_python_helper(test_paths: &[PathBuf], enumerated: &ChangedFileTests) -> bool {
    test_paths.iter().any(|path| {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
            && path.is_file()
            && !python_nodeid_covers(path, &enumerated.python_nodeids)
    })
}

fn unresolved_rust_helper(test_paths: &[PathBuf], enumerated: &ChangedFileTests) -> bool {
    test_paths.iter().any(|path| {
        kiss::Language::is_rust_path(path)
            && path.is_file()
            && !enumerated.rust_tests.iter().any(|(p, _)| p == path)
    })
}

fn python_nodeid_covers(path: &Path, nodeids: &BTreeSet<String>) -> bool {
    nodeids.iter().any(|id| {
        let file = id.split("::").next().unwrap_or(id);
        path.ends_with(file)
    })
}

pub(super) fn append_importer_tests(
    repo_root: &Path,
    ignore: &[String],
    test_paths: &[PathBuf],
    changed: &mut ChangedTestSelectors,
) -> Result<(), String> {
    if test_paths.is_empty() {
        return Ok(());
    }
    let extra = importer_files(repo_root, ignore, test_paths)?;
    if !extra.is_empty() {
        let more = super::super::enumerate_tests_in_changed_files(repo_root, &extra)
            .map_err(|err| err.to_string())?;
        super::extend_tagged(changed, super::changed_file_ids(&more));
    }
    super::extend_tagged(changed, covering_selectors(repo_root, test_paths));
    Ok(())
}

fn importer_files(
    repo_root: &Path,
    ignore: &[String],
    test_paths: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    let root = repo_root.to_string_lossy().to_string();
    let (py, rs) = kiss::gather_files_by_lang(&[root], None, ignore);
    let (py_parsed, rs_parsed, roles) =
        crate::analyze_parse::parse_classified(&py, &rs).map_err(|err| err.to_string())?;
    let mut extra = BTreeSet::new();
    extra.extend(python_importer_paths(&py_parsed, &roles, test_paths));
    extra.extend(rust_importer_paths(&rs_parsed, &roles, test_paths));
    Ok(extra.into_iter().collect())
}

fn python_importer_paths(
    parsed: &[ParsedFile],
    roles: &kiss::code_roles::SourceRoleIndex,
    test_paths: &[PathBuf],
) -> Vec<PathBuf> {
    if parsed.is_empty() {
        return Vec::new();
    }
    let refs: Vec<_> = parsed.iter().collect();
    let ctx = kiss::build_python_context_graph(&refs, roles);
    importer_paths_of(&ctx, test_paths)
}

fn rust_importer_paths(
    parsed: &[ParsedRustFile],
    roles: &kiss::code_roles::SourceRoleIndex,
    test_paths: &[PathBuf],
) -> Vec<PathBuf> {
    if parsed.is_empty() {
        return Vec::new();
    }
    let refs: Vec<_> = parsed.iter().collect();
    let ctx = kiss::build_rust_context_graph(&refs, roles);
    importer_paths_of(&ctx, test_paths)
}

fn importer_paths_of(ctx: &ContextDependencyGraph, test_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut extra = BTreeSet::new();
    for path in test_paths {
        let Some(module) = module_name_for_path(ctx, path) else {
            continue;
        };
        for importer in ctx.test_importers_of(&module) {
            if let Some(importer_path) = path_for_module_name(ctx, &importer) {
                extra.insert(importer_path);
            }
        }
    }
    extra.into_iter().collect()
}

/// Python tests whose recorded coverage reached the changed test files. Rust runs keep
/// no coverage; a changed Rust helper outside any test plans every Rust test instead.
fn covering_selectors(repo_root: &Path, test_paths: &[PathBuf]) -> LanguageKeyed<Vec<String>> {
    let mut ids = LanguageKeyed::<Vec<String>>::default();
    if let Some(sels) =
        crate::test_runner::python_coverage_index::select_python_source_selectors_from_index(
            repo_root, test_paths,
        )
    {
        ids.python.extend(sels);
    }
    ids
}
