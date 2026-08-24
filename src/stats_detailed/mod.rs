mod python;
mod rust;
mod table;
pub mod types;

use crate::graph::DependencyGraph;

pub use python::collect_detailed_py;
pub use rust::{collect_detailed_rs, collect_detailed_rs_with_roles};
pub use table::format_detailed_table;
pub use types::UnitMetrics;

fn module_name_from_path(path: &std::path::Path) -> String {
    path.file_stem()
        .map_or_else(String::new, |s| s.to_str().unwrap_or("").to_string())
}

fn module_id_for_path(path: &std::path::Path, graph: &DependencyGraph) -> String {
    graph
        .paths
        .iter()
        .find_map(|(k, v)| (v == path).then(|| k.clone()))
        .unwrap_or_else(|| module_name_from_path(path))
}

#[derive(Clone, Copy)]
pub(crate) struct FileScopeMetrics {
    pub lines: usize,
    pub imports: Option<usize>,
    pub statements: usize,
    pub functions: usize,
    pub interface_types: usize,
    pub concrete_types: usize,
}

pub(crate) fn file_unit_metrics(
    path: &std::path::Path,
    fm: FileScopeMetrics,
    graph: Option<&DependencyGraph>,
) -> UnitMetrics {
    let (fan_in, fan_out, indirect_deps, dependency_depth) =
        graph.map_or((None, None, None, None), |g| {
            let module_name = module_id_for_path(path, g);
            let m = g.module_metrics(&module_name);
            (
                Some(m.fan_in),
                Some(m.fan_out),
                Some(m.indirect_dependencies),
                Some(m.dependency_depth),
            )
        });
    let name = path
        .file_name()
        .map_or("", |n| n.to_str().unwrap_or(""))
        .to_string();
    let mut u = UnitMetrics::new(path.display().to_string(), name, "file", 1);
    u.lines = Some(fm.lines);
    u.imports = fm.imports;
    u.file_statements = Some(fm.statements);
    u.file_functions = Some(fm.functions);
    u.interface_types = Some(fm.interface_types);
    u.concrete_types = Some(fm.concrete_types);
    u.fan_in = fan_in;
    u.fan_out = fan_out;
    u.indirect_deps = indirect_deps;
    u.dependency_depth = dependency_depth;
    u
}

pub fn truncate(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else if max < 3 {
        s.chars().take(max).collect()
    } else {
        let skip = char_count - (max - 3);
        format!("...{}", s.chars().skip(skip).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DependencyGraph;
    use crate::parsing::{create_parser, parse_file};
    use crate::stats_detailed::python::collect_detailed_from_node_for_test;
    use crate::stats_detailed::rust::{collect_detailed_from_items, get_impl_name};
    use std::io::Write;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this_is_a_very_long_string", 10), "..._string");
    }

    #[test]
    fn test_format_detailed_table() {
        let mut u = UnitMetrics::new("test.rs".to_string(), "foo".to_string(), "function", 1);
        u.statements = Some(5);
        u.arguments = Some(2);
        u.args_positional = Some(2);
        u.args_keyword_only = Some(0);
        u.indentation = Some(1);
        u.nested_depth = Some(0);
        u.branches = Some(0);
        u.returns = Some(1);
        u.locals = Some(3);
        let table = format_detailed_table(&[u]);
        assert!(table.contains("test.rs"));
        assert!(table.contains("foo"));
    }

    #[test]
    fn test_format_detailed_table_renders_missing_metrics_as_dashes() {
        let u = UnitMetrics::new(
            "a/very/long/path/that/exceeds/the/forty/character/column/test.rs".to_string(),
            "a_very_long_function_name".to_string(),
            "function",
            42,
        );

        let table = format_detailed_table(&[u]);

        assert!(table.contains(&truncate(
            "a/very/long/path/that/exceeds/the/forty/character/column/test.rs",
            40
        )));
        assert!(table.contains(&truncate("a_very_long_function_name", 20)));
        assert!(table.contains("     -"));
    }

    #[test]
    fn test_get_impl_name() {
        let code: syn::ItemImpl = syn::parse_quote! { impl Foo { fn bar() {} } };
        assert_eq!(get_impl_name(&code), "Foo");
    }

    #[test]
    fn test_module_name_from_path() {
        use std::path::Path;
        assert_eq!(super::module_name_from_path(Path::new("src/foo.rs")), "foo");
        assert_eq!(super::module_name_from_path(Path::new("bar.py")), "bar");
    }

    #[test]
    fn test_collect_detailed_py_empty() {
        let units = collect_detailed_py(&[], None);
        assert!(units.is_empty());
        let m = super::file_unit_metrics(
            std::path::Path::new("src/foo.py"),
            super::FileScopeMetrics {
                lines: 100,
                imports: Some(5),
                statements: 0,
                functions: 0,
                interface_types: 0,
                concrete_types: 0,
            },
            None,
        );
        assert_eq!(m.name, "foo.py");
        assert_eq!(m.lines, Some(100));
    }

    #[test]
    fn test_file_unit_metrics_uses_graph_module_id_for_path() {
        let mut g = DependencyGraph::new();
        let p = std::path::PathBuf::from("src/pkg/foo.py");
        g.paths.insert("pkg.foo".to_string(), p.clone());
        g.get_or_create_node("pkg.foo");
        g.add_dependency("pkg.foo", "bar");

        let m = super::file_unit_metrics(
            &p,
            super::FileScopeMetrics {
                lines: 10,
                imports: Some(0),
                statements: 0,
                functions: 0,
                interface_types: 0,
                concrete_types: 0,
            },
            Some(&g),
        );
        assert_eq!(m.fan_out, Some(1), "expected metrics from pkg.foo node");
    }

    #[test]
    fn test_collect_detailed_rs_empty() {
        let units = collect_detailed_rs(&[], None);
        assert!(units.is_empty());
    }

    #[test]
    fn test_collect_detailed_from_node() {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(
            tmp,
            "def foo():\n    x = 1\nclass Bar:\n    def m(self): pass"
        )
        .unwrap();
        let parsed = parse_file(&mut create_parser().unwrap(), tmp.path()).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_node_for_test(
            parsed.tree.root_node(),
            &parsed.source,
            "t.py",
            &mut units,
        );
        assert!(
            units
                .iter()
                .any(|u| u.name == "foo" && u.kind == "function")
        );
        assert!(units.iter().any(|u| u.name == "Bar" && u.kind == "class"));
    }

    #[test]
    fn test_collect_detailed_from_items() {
        let code: syn::File =
            syn::parse_quote! { fn foo() { let x = 1; } impl Bar { fn m(&self) {} } };
        let mut units = Vec::new();
        collect_detailed_from_items(&code.items, "t.rs", &mut units);
        assert!(
            units
                .iter()
                .any(|u| u.name == "foo" && u.kind == "function")
        );
        assert!(units.iter().any(|u| u.name == "Bar" && u.kind == "impl"));
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use std::path::Path;

    impl FileScopeMetrics {
        fn witness() -> Self {
            Self {
                lines: 1,
                imports: Some(0),
                statements: 0,
                functions: 0,
                interface_types: 0,
                concrete_types: 0,
            }
        }
    }

    #[test]
    fn witness_file_scope_metrics() {
        let fm = FileScopeMetrics::witness();
        let unit = file_unit_metrics(Path::new("a.py"), fm, None);
        assert_eq!(unit.lines, Some(1));
    }
}
