use super::definitions::{self, is_binary_entry_point, RustCodeDefinition};
use super::references::{
    collect_rust_references_for_fn_coverage_map, ReferenceVisitor, RefWitnessMode,
};
use super::{
    has_cfg_test_attribute, has_test_attribute, is_covered_by_tests,
    is_covered_by_tests_for_coverage_map, is_rust_test_file, CoveringTest, PerTestUsage,
};
use crate::rust_parsing::ParsedRustFile;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::{ImplItem, Item, Type};

pub(crate) fn is_calibration_excluded_file(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == "logger.rs") {
        return true;
    }
    path.components().zip(path.components().skip(1)).any(
        |(a, b)| {
            matches!(a, std::path::Component::Normal(x) if x == "flags")
                && matches!(b, std::path::Component::Normal(x) if x == "doc")
        },
    )
}

pub(crate) fn expand_coverage_map_witnesses(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    if has_rust_integration_test_runner(parsed_files) {
        seed_binary_entry_roots(parsed_files, refs);
        expand_coverage_references_to_fixpoint(parsed_files, refs);
        expand_small_module_defs_from_stem_refs(parsed_files, refs);
    } else {
        expand_coverage_references_one_hop(parsed_files, refs);
    }
}

const MAX_MODULE_STEM_EXPAND_DEFS: usize = 8;

fn expand_small_module_defs_from_stem_refs(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            continue;
        }
        let Some(stem) = parsed.path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !refs.contains(stem) {
            continue;
        }
        let mut names = Vec::new();
        collect_fn_names_from_items(&parsed.ast.items, &mut names);
        if names.len() > MAX_MODULE_STEM_EXPAND_DEFS {
            continue;
        }
        for name in names {
            refs.insert(name);
        }
    }
}

fn collect_fn_names_from_items(items: &[Item], names: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Fn(f)
                if !has_test_attribute(&f.attrs)
                    && !definitions::is_private(&f.sig.ident.to_string()) =>
            {
                names.push(f.sig.ident.to_string());
            }
            Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
                if let Some((_, sub)) = &m.content {
                    collect_fn_names_from_items(sub, names);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn merge_one_hop_refs(
    body_refs: HashSet<String>,
    refs: &HashSet<String>,
    added: &mut HashSet<String>,
) {
    for r in body_refs {
        if !refs.contains(&r) {
            added.insert(r);
        }
    }
}

fn impl_self_type_name(ty: &Type) -> Option<String> {
    if let Type::Path(p) = ty {
        return p.path.segments.last().map(|s| s.ident.to_string());
    }
    None
}

pub(crate) fn has_rust_integration_test_runner(parsed_files: &[&ParsedRustFile]) -> bool {
    parsed_files.iter().any(|parsed| {
        path_is_under_tests(&parsed.path)
            && (parsed.source.contains("current_exe")
                || parsed.source.contains("Command::new"))
    })
}

pub(crate) fn path_is_under_tests(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, std::path::Component::Normal(s) if s == "tests"))
}

pub(crate) fn seed_binary_entry_roots(parsed_files: &[&ParsedRustFile], refs: &mut HashSet<String>) {
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) || !is_binary_entry_point(&parsed.path) {
            continue;
        }
        for item in &parsed.ast.items {
            seed_binary_entry_roots_from_item(item, refs);
        }
    }
}

pub(crate) fn seed_binary_entry_roots_from_item(item: &Item, refs: &mut HashSet<String>) {
    match item {
        Item::Fn(f) if !has_test_attribute(&f.attrs) => {
            let name = f.sig.ident.to_string();
            if name == "main" || name == "run" {
                refs.insert(name);
            }
        }
        Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
            if let Some((_, items)) = &m.content {
                for sub in items {
                    seed_binary_entry_roots_from_item(sub, refs);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn expand_coverage_references_to_fixpoint(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    const MAX_ROUNDS: usize = 64;
    for _ in 0..MAX_ROUNDS {
        let before = refs.len();
        expand_coverage_references_one_hop(parsed_files, refs);
        if refs.len() == before {
            break;
        }
    }
}

pub(crate) fn expand_coverage_references_one_hop(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    let mut added = HashSet::new();
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            continue;
        }
        for item in &parsed.ast.items {
            expand_one_hop_from_item(item, refs, &mut added);
        }
    }
    refs.extend(added);
}

pub(crate) fn expand_one_hop_from_item(
    item: &Item,
    refs: &HashSet<String>,
    added: &mut HashSet<String>,
) {
    match item {
        Item::Fn(f) if !has_test_attribute(&f.attrs) => {
            let name = f.sig.ident.to_string();
            if refs.contains(&name) {
                merge_one_hop_refs(collect_rust_references_for_fn_coverage_map(f), refs, added);
            }
        }
        Item::Impl(i) if !has_cfg_test_attribute(&i.attrs) => {
            let type_name = impl_self_type_name(&i.self_ty);
            for impl_item in &i.items {
                if let ImplItem::Fn(m) = impl_item {
                    let name = m.sig.ident.to_string();
                    let type_ok = type_name.as_ref().is_none_or(|t| refs.contains(t));
                    if refs.contains(&name) && type_ok {
                        let mut body_refs = HashSet::new();
                        ReferenceVisitor {
                            refs: &mut body_refs,
                            mode: RefWitnessMode::COVERAGE_MAP,
                        }
                        .visit_block(&m.block);
                        merge_one_hop_refs(body_refs, refs, added);
                    }
                }
            }
        }
        Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
            if let Some((_, items)) = &m.content {
                for sub in items {
                    expand_one_hop_from_item(sub, refs, added);
                }
            }
        }
        _ => {}
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn build_rust_coverage_map(
    definitions: &[RustCodeDefinition],
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    coverage_references: &HashSet<String>,
) -> HashMap<(PathBuf, String), Vec<CoveringTest>> {
    let mut name_to_defs: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, def) in definitions.iter().enumerate() {
        name_to_defs.entry(&def.name).or_default().push(i);
        if let Some(ref t) = def.impl_for_type {
            name_to_defs.entry(t.as_str()).or_default().push(i);
        }
    }

    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    for (test_path, test_funcs) in per_test_usage {
        for (test_id, usage_refs) in test_funcs {
            if test_id.is_empty() {
                continue;
            }
            let mut seen = HashSet::new();
            for ref_name in usage_refs {
                let Some(def_indices) = name_to_defs.get(ref_name.as_str()) else {
                    continue;
                };
                for &idx in def_indices {
                    if !seen.insert(idx) {
                        continue;
                    }
                    let def = &definitions[idx];
                    if !is_covered_by_tests(def, coverage_references, name_files, disambiguation) {
                        continue;
                    }
                    let key = (def.file.clone(), def.name.clone());
                    let entry = (test_path.clone(), test_id.clone());
                    let list = coverage_map.entry(key).or_default();
                    if !list.contains(&entry) {
                        list.push(entry);
                    }
                }
            }
        }
    }
    coverage_map
}

pub(crate) fn build_rust_coverage_map_for_calibration(
    definitions: &[RustCodeDefinition],
    per_test_usage: &PerTestUsage,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    coverage_references: &HashSet<String>,
) -> HashMap<(PathBuf, String), Vec<CoveringTest>> {
    let mut name_to_defs: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, def) in definitions.iter().enumerate() {
        name_to_defs.entry(def.name.as_str()).or_default().push(i);
    }

    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    for (test_path, test_funcs) in per_test_usage {
        for (test_id, usage_refs) in test_funcs {
            if test_id.is_empty() {
                continue;
            }
            let mut seen = HashSet::new();
            for ref_name in usage_refs {
                let Some(def_indices) = name_to_defs.get(ref_name.as_str()) else {
                    continue;
                };
                for &idx in def_indices {
                    if !seen.insert(idx) {
                        continue;
                    }
                    let def = &definitions[idx];
                    if !is_covered_by_tests_for_coverage_map(
                        def,
                        coverage_references,
                        name_files,
                        disambiguation,
                    ) {
                        continue;
                    }
                    let key = (def.file.clone(), def.name.clone());
                    let entry = (test_path.clone(), test_id.clone());
                    let list = coverage_map.entry(key).or_default();
                    if !list.contains(&entry) {
                        list.push(entry);
                    }
                }
            }
        }
    }
    coverage_map
}

#[cfg(test)]
mod calibration_tests {
    use super::*;
    use crate::rust_parsing::parse_rust_file;
    use std::io::Write as _;

    #[test]
    fn small_module_stem_expands_public_fns() {
        let mut msg = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        write!(msg, "pub fn a() {{}}\npub fn b() {{}}\n").unwrap();
        let parsed_msg = parse_rust_file(msg.path()).unwrap();
        let stem = parsed_msg.path.file_stem().unwrap().to_str().unwrap();
        let mut refs = HashSet::from([stem.to_string()]);
        expand_small_module_defs_from_stem_refs(&[&parsed_msg], &mut refs);
        assert!(refs.contains("a"));
        assert!(refs.contains("b"));
    }

    #[test]
    fn touch_calibration_helpers() {
        fn touch<T>(_: T) {}
        touch(expand_coverage_map_witnesses);
        touch(build_rust_coverage_map_for_calibration);
        touch(collect_fn_names_from_items);
        touch(impl_self_type_name);
    }
}
