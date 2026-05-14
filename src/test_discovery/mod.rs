pub mod args;

use crate::analyze;
use kiss::DependencyGraph;
use kiss::Language;
use kiss::graph::build_dependency_graph;
use kiss::rust_graph::build_rust_dependency_graph;
use kiss::test_refs::CoveringTest;
use kiss::{ParsedFile, ParsedRustFile};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub type DefEntry = (PathBuf, String, usize, Option<Vec<CoveringTest>>);

fn gather_files_with_path_expansion(
    universe: &str,
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let universe_root = Path::new(universe);
    let (py_files, rs_files) = analyze::gather_files(universe_root, lang_filter, ignore);
    let mut all_py: HashSet<PathBuf> = py_files.into_iter().collect();
    let mut all_rs: HashSet<PathBuf> = rs_files.into_iter().collect();
    for path_str in paths {
        let path = Path::new(path_str);
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        let root = if canonical.is_dir() {
            canonical
        } else {
            match canonical.parent() {
                Some(p) => p.to_path_buf(),
                None => continue,
            }
        };
        let (py, rs) = analyze::gather_files(&root, lang_filter, ignore);
        all_py.extend(py);
        all_rs.extend(rs);
    }
    let mut py_files: Vec<PathBuf> = all_py.into_iter().collect();
    let mut rs_files: Vec<PathBuf> = all_rs.into_iter().collect();
    py_files.sort();
    rs_files.sort();
    (py_files, rs_files)
}

pub fn discover_covering_tests(
    a: args::DiscoverArgs<'_>,
) -> Result<Vec<DefEntry>, String> {
    let args::DiscoverArgs {
        universe,
        paths,
        lang_filter,
        ignore,
    } = a;
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let (py_files, rs_files) =
        gather_files_with_path_expansion(universe, paths, lang_filter, ignore);
    let focus_set = analyze::build_focus_set(paths, lang_filter, ignore);
    if focus_set.is_empty() {
        return Ok(Vec::new());
    }
    let mut all_defs: Vec<DefEntry> = Vec::new();
    if !py_files.is_empty() {
        let (defs, _) = collect_py_test_defs(&py_files, &focus_set)?;
        all_defs.extend(defs);
    }
    if !rs_files.is_empty() {
        let (defs, _) = collect_rs_test_defs(&rs_files, &focus_set);
        all_defs.extend(defs);
    }
    all_defs.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    Ok(all_defs)
}

pub(crate) fn defs_from_analysis_rows(
    definitions: impl Iterator<Item = (PathBuf, String, usize)>,
    unreferenced: impl Iterator<Item = (PathBuf, String, usize)>,
    coverage_map: &HashMap<(PathBuf, String), Vec<CoveringTest>>,
    focus_set: &HashSet<PathBuf>,
) -> Vec<DefEntry> {
    let unref_set: HashSet<(PathBuf, String, usize)> = unreferenced.collect();
    definitions
        .filter(|(file, _, _)| analyze::is_focus_file(file, focus_set))
        .map(|(file, name, line)| {
            let key = (file.clone(), name.clone());
            let covering = if unref_set.contains(&(file.clone(), name.clone(), line)) {
                None
            } else {
                Some(coverage_map.get(&key).cloned().unwrap_or_default())
            };
            (file, name, line, covering)
        })
        .collect()
}

macro_rules! defs_from_test_ref_analysis {
    ($analysis:expr, $focus_set:expr) => {
        defs_from_analysis_rows(
            $analysis
                .definitions
                .iter()
                .map(|d| (d.file.clone(), d.name.clone(), d.line)),
            $analysis
                .unreferenced
                .iter()
                .map(|d| (d.file.clone(), d.name.clone(), d.line)),
            &$analysis.coverage_map,
            $focus_set,
        )
    };
}

fn collect_py_test_defs(
    py_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
) -> Result<(Vec<DefEntry>, Option<DependencyGraph>), String> {
    let results = kiss::parse_files(py_files).map_err(|e| e.to_string())?;
    let parsed: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let graph = if refs.is_empty() {
        None
    } else {
        Some(build_dependency_graph(&refs))
    };
    let analysis = kiss::analyze_test_refs(&refs, graph.as_ref());
    let defs = defs_from_test_ref_analysis!(&analysis, focus_set);
    Ok((defs, graph))
}

fn collect_rs_test_defs(
    rs_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
) -> (Vec<DefEntry>, Option<DependencyGraph>) {
    let results = kiss::parse_rust_files(rs_files);
    let parsed: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let graph = if refs.is_empty() {
        None
    } else {
        Some(build_rust_dependency_graph(&refs))
    };
    let analysis = kiss::analyze_rust_test_refs(&refs, graph.as_ref());
    let defs = defs_from_test_ref_analysis!(&analysis, focus_set);
    (defs, graph)
}

#[cfg(test)]
#[path = "test_discovery_test.rs"]
mod tests;
