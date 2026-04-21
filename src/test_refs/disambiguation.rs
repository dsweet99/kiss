use crate::graph::DependencyGraph;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub fn build_name_file_map<'a>(
    items: impl Iterator<Item = (&'a str, &'a Path)>,
) -> HashMap<String, HashSet<PathBuf>> {
    let mut map: HashMap<String, HashSet<PathBuf>> = HashMap::new();
    for (name, file) in items {
        map.entry(name.to_string())
            .or_default()
            .insert(file.to_path_buf());
    }
    map
}

pub(crate) fn path_identifiers(file: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(parent) = file.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(os) = component
                && let Some(s) = os.to_str()
            {
                ids.push(s.to_string());
            }
        }
    }
    if let Some(stem) = file.file_stem().and_then(|s| s.to_str()) {
        ids.push(stem.to_string());
    }
    ids
}

pub(crate) fn disambiguate_files(
    files: &HashSet<PathBuf>,
    refs: &HashSet<String>,
) -> Option<PathBuf> {
    let file_ids: Vec<(&PathBuf, Vec<String>)> =
        files.iter().map(|f| (f, path_identifiers(f))).collect();

    let mut id_file_count: HashMap<&str, usize> = HashMap::new();
    for (_, ids) in &file_ids {
        for id in ids {
            *id_file_count.entry(id.as_str()).or_default() += 1;
        }
    }

    let mut winner: Option<&PathBuf> = None;
    for (file, ids) in &file_ids {
        let has_unique = ids
            .iter()
            .any(|id| refs.contains(id) && id_file_count.get(id.as_str()).copied() == Some(1));
        if has_unique {
            if winner.is_some() {
                return None;
            }
            winner = Some(file);
        }
    }
    winner.cloned()
}

#[allow(clippy::type_complexity)]
pub(crate) fn disambiguate_files_graph_fallback(
    files: &HashSet<PathBuf>,
    test_files_using_name: &[PathBuf],
    graph: &DependencyGraph,
) -> Option<PathBuf> {
    if test_files_using_name.is_empty() {
        return None;
    }
    let candidate_modules: Vec<(PathBuf, String)> = files
        .iter()
        .filter_map(|f| graph.module_for_path(f).map(|m| (f.clone(), m)))
        .collect();
    if candidate_modules.is_empty() || candidate_modules.len() != files.len() {
        return None;
    }
    let mut winner: Option<PathBuf> = None;
    for (cand_path, cand_module) in &candidate_modules {
        let has_importer = test_files_using_name.iter().any(|test_path| {
            graph
                .module_for_path(test_path)
                .is_some_and(|test_mod| graph.imports(test_mod.as_str(), cand_module))
        });
        if has_importer {
            if winner.is_some() {
                return None;
            }
            winner = Some(cand_path.clone());
        }
    }
    winner
}

pub(crate) fn resolve_ambiguous_name(
    name: &str,
    files: &HashSet<PathBuf>,
    refs: &HashSet<String>,
    name_to_test_files: &HashMap<&str, Vec<PathBuf>>,
    graph: Option<&DependencyGraph>,
) -> Option<PathBuf> {
    disambiguate_files(files, refs).or_else(|| {
        let g = graph?;
        let empty = Vec::new();
        let test_files = name_to_test_files.get(name).unwrap_or(&empty);
        disambiguate_files_graph_fallback(files, test_files, g)
    })
}

#[allow(clippy::type_complexity)]
fn collect_test_files_for_ambiguous_names<'a>(
    per_test_usage: &'a [(PathBuf, Vec<(String, HashSet<String>)>)],
    name_files: &HashMap<String, HashSet<PathBuf>>,
) -> HashMap<&'a str, Vec<PathBuf>> {
    let mut map: HashMap<&str, Vec<PathBuf>> = HashMap::new();
    for (test_path, test_funcs) in per_test_usage {
        for (_, usage_refs) in test_funcs {
            for ref_name in usage_refs {
                if name_files.get(ref_name).is_some_and(|f| f.len() > 1) {
                    let entry = map.entry(ref_name.as_str()).or_default();
                    if entry.last() != Some(test_path) {
                        entry.push(test_path.clone());
                    }
                }
            }
        }
    }
    map
}

#[allow(clippy::type_complexity)]
pub(crate) fn build_disambiguation_map(
    name_files: &HashMap<String, HashSet<PathBuf>>,
    refs: &HashSet<String>,
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    graph: Option<&DependencyGraph>,
) -> HashMap<String, PathBuf> {
    let name_to_test_files = if graph.is_some() {
        collect_test_files_for_ambiguous_names(per_test_usage, name_files)
    } else {
        HashMap::new()
    };

    name_files
        .iter()
        .filter(|(_, files)| files.len() > 1)
        .filter_map(|(name, files)| {
            resolve_ambiguous_name(name, files, refs, &name_to_test_files, graph)
                .map(|f| (name.clone(), f))
        })
        .collect()
}

pub(crate) fn file_to_module_suffix(file: &Path) -> String {
    let mut parts = Vec::new();
    if let Some(parent) = file.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(os) = component
                && let Some(s) = os.to_str()
            {
                parts.push(s);
            }
        }
    }
    if let Some(stem) = file.file_stem().and_then(|s| s.to_str()) {
        parts.push(stem);
    }
    parts.join(".")
}

pub(crate) fn module_suffix_matches(def_suffix: &str, import_module: &str) -> bool {
    def_suffix == import_module || def_suffix.ends_with(&format!(".{import_module}"))
}
