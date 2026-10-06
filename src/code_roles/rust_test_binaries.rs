use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::rust_include::canonical_path;

use super::cfg_pred::{AtomInterner, CfgPred};
use super::error::RoleBuildError;
use super::rust_cargo::{CargoRoot, workspace_roots_at};
use super::rust_modules::resolve_external_mod;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RustTestBinaryModule {
    pub binary_prefix: String,
    pub module_path: String,
}

pub fn workspace_rust_test_modules(
    repo_root: &Path,
) -> Result<HashMap<PathBuf, Vec<RustTestBinaryModule>>, RoleBuildError> {
    modules_by_file(repo_root, |root| {
        let binary_prefix = libtest_binary_prefix(root)?;
        Some(move |module_path| RustTestBinaryModule {
            binary_prefix: binary_prefix.clone(),
            module_path,
        })
    })
}

pub fn workspace_nextest_file_modules(
    repo_root: &Path,
) -> Result<HashMap<PathBuf, Vec<(String, String)>>, RoleBuildError> {
    modules_by_file(repo_root, |root| {
        libtest_binary_prefix(root)?;
        let binary_id = nextest_binary_id(root);
        Some(move |module_path| (binary_id.clone(), module_path))
    })
}

fn modules_by_file<T: Ord, F: Fn(String) -> T>(
    repo_root: &Path,
    module_of: impl Fn(&CargoRoot) -> Option<F>,
) -> Result<HashMap<PathBuf, Vec<T>>, RoleBuildError> {
    let mut out: HashMap<PathBuf, Vec<T>> = HashMap::new();
    for root in workspace_roots_at(repo_root)? {
        let Some(make) = module_of(&root) else {
            continue;
        };
        let mut modules = Vec::new();
        collect_file_modules(
            &root.src_path,
            String::new(),
            &mut HashSet::new(),
            &mut modules,
        );
        for (file, module_path) in modules {
            let entry = out.entry(file).or_default();
            let module = make(module_path);
            if !entry.contains(&module) {
                entry.push(module);
            }
        }
    }
    for modules in out.values_mut() {
        modules.sort();
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextestBinary {
    pub selector_prefix: String,
    pub binary_id: String,
    pub src_path: PathBuf,
}

pub fn workspace_nextest_binaries(repo_root: &Path) -> Result<Vec<NextestBinary>, RoleBuildError> {
    Ok(workspace_roots_at(repo_root)?
        .iter()
        .filter_map(|root| {
            Some(NextestBinary {
                selector_prefix: libtest_binary_prefix(root)?,
                binary_id: nextest_binary_id(root),
                src_path: root.src_path.clone(),
            })
        })
        .collect())
}

pub fn workspace_nextest_binary_ids(
    repo_root: &Path,
) -> Result<HashMap<String, Vec<String>>, RoleBuildError> {
    let mut ids: HashMap<String, Vec<String>> = HashMap::new();
    for binary in workspace_nextest_binaries(repo_root)? {
        let found = ids.entry(binary.selector_prefix).or_default();
        if !found.contains(&binary.binary_id) {
            found.push(binary.binary_id);
            found.sort();
        }
    }
    Ok(ids)
}

fn nextest_binary_id(root: &CargoRoot) -> String {
    let kind_prefix = ["bin", "example", "bench"]
        .into_iter()
        .find(|kind| root.kinds.iter().any(|k| k == kind));
    match kind_prefix {
        Some(kind) => format!("{}::{kind}/{}", root.package, root.name),
        None if root.kinds.iter().any(|k| k == "test") => {
            format!("{}::{}", root.package, root.name)
        }
        None => root.package.clone(),
    }
}

fn libtest_binary_prefix(root: &CargoRoot) -> Option<String> {
    if root.kinds.iter().all(|kind| kind == "custom-build") {
        return None;
    }
    Some(format!("{}::{}", root.package, root.name))
}

fn collect_file_modules(
    file: &Path,
    module_path: String,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<(PathBuf, String)>,
) {
    let file = canonical_path(file);
    if !seen.insert(file.clone()) {
        return;
    }
    let Ok(source) = std::fs::read_to_string(&file) else {
        return;
    };
    let Ok(ast) = syn::parse_file(&source) else {
        return;
    };
    out.push((file.clone(), module_path.clone()));
    collect_item_modules(&file, &ast.items, &module_path, seen, out);
}

fn collect_item_modules(
    file: &Path,
    items: &[syn::Item],
    module_path: &str,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<(PathBuf, String)>,
) {
    for item in items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        let child_path = join_module(module_path, &module.ident.to_string());
        if let Some((_, nested)) = &module.content {
            collect_item_modules(file, nested, &child_path, seen, out);
            continue;
        }
        let mut atoms = AtomInterner::new();
        let Ok(edges) = resolve_external_mod(file, module, &CfgPred::True, &mut atoms) else {
            continue;
        };
        for edge in edges {
            collect_file_modules(&edge.target, child_path.clone(), seen, out);
        }
    }
}

fn join_module(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}::{name}")
    }
}

#[cfg(test)]
mod rust_test_binaries_test {
    use super::*;

    #[test]
    fn maps_unit_and_integration_files_to_binaries_and_module_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo-pkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub mod a;\n").unwrap();
        std::fs::write(root.join("src/a.rs"), "").unwrap();
        std::fs::write(root.join("src/main.rs"), "mod a;\n").unwrap();
        std::fs::write(root.join("tests/it.rs"), "").unwrap();

        let map = workspace_rust_test_modules(root).unwrap();
        let get = |rel: &str| {
            map.get(&canonical_path(&root.join(rel)))
                .cloned()
                .unwrap_or_default()
        };
        let module = |binary_prefix: &str, module_path: &str| RustTestBinaryModule {
            binary_prefix: binary_prefix.to_string(),
            module_path: module_path.to_string(),
        };

        assert_eq!(get("src/lib.rs"), [module("demo-pkg::demo_pkg", "")]);
        assert_eq!(
            get("src/a.rs"),
            [
                module("demo-pkg::demo-pkg", "a"),
                module("demo-pkg::demo_pkg", "a")
            ]
        );
        assert_eq!(get("tests/it.rs"), [module("demo-pkg::it", "")]);

        let files = super::workspace_nextest_file_modules(root).unwrap();
        let in_binaries = |rel: &str| {
            files
                .get(&canonical_path(&root.join(rel)))
                .cloned()
                .unwrap_or_default()
        };
        let pair = |id: &str, module: &str| (id.to_string(), module.to_string());
        assert_eq!(
            in_binaries("src/a.rs"),
            [pair("demo-pkg", "a"), pair("demo-pkg::bin/demo-pkg", "a")]
        );
        assert_eq!(in_binaries("tests/it.rs"), [pair("demo-pkg::it", "")]);

        let ids = super::workspace_nextest_binary_ids(root).unwrap();
        assert_eq!(ids["demo-pkg::demo_pkg"], ["demo-pkg"]);
        assert_eq!(ids["demo-pkg::demo-pkg"], ["demo-pkg::bin/demo-pkg"]);
        assert_eq!(ids["demo-pkg::it"], ["demo-pkg::it"]);
    }
}
