use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::test_runner::coverage_decision::TestSelector;
use kiss::{
    ContextDependencyGraph, ParsedFile, ParsedRustFile, module_name_for_path, path_for_module_name,
};

use super::super::ChangedFileTests;

pub(super) fn expand_unresolved_test_helpers(
    repo_root: &Path,
    test_paths: &[PathBuf],
    ignore: &[String],
    enumerated: &ChangedFileTests,
    python: &mut Vec<TestSelector>,
    rust: &mut Vec<TestSelector>,
) -> Result<(), String> {
    if unresolved_python_helper(test_paths, enumerated) {
        for selector in super::super::enumerate_workspace_python_selectors(repo_root, ignore, &[])?
        {
            python.push(TestSelector::new(kiss::Language::Python, selector));
        }
    }
    if unresolved_rust_helper(test_paths, enumerated) {
        for selector in super::super::enumerate_workspace_rust_selectors(repo_root, ignore)? {
            rust.push(TestSelector::new(kiss::Language::Rust, selector));
        }
    }
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
    python: &mut Vec<TestSelector>,
    rust: &mut Vec<TestSelector>,
) -> Result<(), String> {
    if test_paths.is_empty() {
        return Ok(());
    }
    let extra = importer_files(repo_root, ignore, test_paths)?;
    if extra.is_empty() {
        append_covering_selectors(repo_root, test_paths, python, rust);
        return Ok(());
    }
    let more = super::super::enumerate_tests_in_changed_files(repo_root, &extra)
        .map_err(|err| err.to_string())?;
    for nodeid in more.python_nodeids {
        python.push(TestSelector::new(kiss::Language::Python, nodeid));
    }
    for (path, id) in more.rust_tests {
        if kiss::Language::is_rust_path(&path) {
            rust.push(TestSelector::new(kiss::Language::Rust, id));
        }
    }
    append_covering_selectors(repo_root, test_paths, python, rust);
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

fn append_covering_selectors(
    repo_root: &Path,
    test_paths: &[PathBuf],
    python: &mut Vec<TestSelector>,
    rust: &mut Vec<TestSelector>,
) {
    if let Some(sels) =
        crate::test_runner::python_coverage_index::select_python_source_selectors_from_index(
            repo_root, test_paths,
        )
    {
        for id in sels {
            python.push(TestSelector::new(kiss::Language::Python, id));
        }
    }
    if let Some(pop) = crate::test_runner::rust_coverage_index::load_current_rust_population_state(
        repo_root,
        None,
        &[],
    ) && let Some(sels) = crate::test_runner::rust_coverage_index::selectors_for_source_paths(
        repo_root,
        test_paths,
        &pop.line_index,
    ) {
        for id in sels {
            rust.push(TestSelector::new(kiss::Language::Rust, id));
        }
    }
}
