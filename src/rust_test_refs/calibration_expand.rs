use super::calibration_map::is_coverage_map_rule_settings_file;
use super::{has_cfg_test_attribute, has_test_attribute, is_rust_test_file};
use crate::rust_parsing::ParsedRustFile;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use syn::Item;

const MAX_MODULE_STEM_EXPAND_DEFS: usize = 8;

pub(crate) fn expand_witnessed_directory_sibling_defs(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    let mut by_dir: BTreeMap<PathBuf, Vec<&ParsedRustFile>> = BTreeMap::new();
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            continue;
        }
        let Some(parent) = parsed.path.parent() else {
            continue;
        };
        by_dir
            .entry(crate::rust_include::canonical_path(parent))
            .or_default()
            .push(parsed);
    }
    for files in by_dir.values() {
        let dir_has_stem_witness = files.iter().any(|p| {
            p.path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| refs.contains(stem))
        });
        if !dir_has_stem_witness {
            continue;
        }
        for parsed in files {
            if is_coverage_map_rule_settings_file(&parsed.path) {
                continue;
            }
            let Some(stem) = parsed.path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if refs.contains(stem) {
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
}

pub(crate) fn expand_small_module_defs_from_stem_refs(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) || is_coverage_map_rule_settings_file(&parsed.path) {
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

pub(crate) fn collect_fn_names_from_items(items: &[Item], names: &mut Vec<String>) {
    use super::definitions;
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
