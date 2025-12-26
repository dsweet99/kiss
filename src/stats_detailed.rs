
use crate::graph::DependencyGraph;
use crate::parsing::ParsedFile;
use crate::py_metrics::{compute_class_metrics_with_source, compute_file_metrics, compute_function_metrics};
use crate::rust_fn_metrics::{compute_rust_file_metrics, compute_rust_function_metrics};
use crate::rust_parsing::ParsedRustFile;
use syn::{ImplItem, Item};
use tree_sitter::Node;

#[derive(Debug, Clone)]
pub struct UnitMetrics {
    pub file: String,
    pub name: String,
    pub kind: &'static str,
    pub line: usize,
    pub statements: Option<usize>,
    pub arguments: Option<usize>,
    pub indentation: Option<usize>,
    pub branches: Option<usize>,
    pub returns: Option<usize>,
    pub locals: Option<usize>,
    pub methods: Option<usize>,
    pub lines: Option<usize>,
    pub imports: Option<usize>,
    pub fan_in: Option<usize>,
    pub fan_out: Option<usize>,
}

fn module_name_from_path(path: &std::path::Path) -> String {
    path.file_stem().map_or_else(String::new, |s| s.to_str().unwrap_or("").to_string())
}

pub fn collect_detailed_py(parsed_files: &[&ParsedFile], graph: Option<&DependencyGraph>) -> Vec<UnitMetrics> {
    let mut units = Vec::new();
    for parsed in parsed_files {
        let file = parsed.path.display().to_string();
        let fm = compute_file_metrics(parsed);
        let module_name = module_name_from_path(&parsed.path);
        let (fan_in, fan_out) = graph.map_or((None, None), |g| { let m = g.module_metrics(&module_name); (Some(m.fan_in), Some(m.fan_out)) });
        units.push(UnitMetrics { file: file.clone(), name: parsed.path.file_name().map_or("", |n| n.to_str().unwrap_or("")).to_string(), kind: "file", line: 1, statements: None, arguments: None, indentation: None, branches: None, returns: None, locals: None, methods: None, lines: Some(fm.lines), imports: Some(fm.imports), fan_in, fan_out });
        collect_detailed_from_node(parsed.tree.root_node(), &parsed.source, &file, &mut units);
    }
    units
}

fn collect_detailed_from_node(node: Node, source: &str, file: &str, units: &mut Vec<UnitMetrics>) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = node.child_by_field_name("name").and_then(|n| n.utf8_text(source.as_bytes()).ok()).unwrap_or("?");
            let m = compute_function_metrics(node, source);
            units.push(UnitMetrics { file: file.to_string(), name: name.to_string(), kind: "function", line: node.start_position().row + 1, statements: Some(m.statements), arguments: Some(m.arguments), indentation: Some(m.max_indentation), branches: Some(m.branches), returns: Some(m.returns), locals: Some(m.local_variables), methods: None, lines: None, imports: None, fan_in: None, fan_out: None });
        }
        "class_definition" => {
            let name = node.child_by_field_name("name").and_then(|n| n.utf8_text(source.as_bytes()).ok()).unwrap_or("?");
            let m = compute_class_metrics_with_source(node, source);
            units.push(UnitMetrics { file: file.to_string(), name: name.to_string(), kind: "class", line: node.start_position().row + 1, statements: None, arguments: None, indentation: None, branches: None, returns: None, locals: None, methods: Some(m.methods), lines: None, imports: None, fan_in: None, fan_out: None });
        }
        _ => {}
    }
    let mut c = node.walk();
    for child in node.children(&mut c) { collect_detailed_from_node(child, source, file, units); }
}

pub fn collect_detailed_rs(parsed_files: &[&ParsedRustFile], graph: Option<&DependencyGraph>) -> Vec<UnitMetrics> {
    let mut units = Vec::new();
    for parsed in parsed_files {
        let file = parsed.path.display().to_string();
        let fm = compute_rust_file_metrics(parsed);
        let module_name = module_name_from_path(&parsed.path);
        let (fan_in, fan_out) = graph.map_or((None, None), |g| { let m = g.module_metrics(&module_name); (Some(m.fan_in), Some(m.fan_out)) });
        units.push(UnitMetrics { file: file.clone(), name: parsed.path.file_name().map_or("", |n| n.to_str().unwrap_or("")).to_string(), kind: "file", line: 1, statements: None, arguments: None, indentation: None, branches: None, returns: None, locals: None, methods: None, lines: Some(fm.lines), imports: Some(fm.imports), fan_in, fan_out });
        collect_detailed_from_items(&parsed.ast.items, &file, &mut units);
    }
    units
}

fn collect_detailed_from_items(items: &[Item], file: &str, units: &mut Vec<UnitMetrics>) {
    for item in items {
        match item {
            Item::Fn(f) => {
                let m = compute_rust_function_metrics(&f.sig.inputs, &f.block, f.attrs.len());
                units.push(UnitMetrics { file: file.to_string(), name: f.sig.ident.to_string(), kind: "function", line: f.sig.ident.span().start().line, statements: Some(m.statements), arguments: Some(m.arguments), indentation: Some(m.max_indentation), branches: Some(m.branches), returns: Some(m.returns), locals: Some(m.local_variables), methods: None, lines: None, imports: None, fan_in: None, fan_out: None });
            }
            Item::Impl(i) => {
                let name = get_impl_name(i);
                let mcnt = i.items.iter().filter(|ii| matches!(ii, ImplItem::Fn(_))).count();
                units.push(UnitMetrics { file: file.to_string(), name, kind: "impl", line: i.impl_token.span.start().line, statements: None, arguments: None, indentation: None, branches: None, returns: None, locals: None, methods: Some(mcnt), lines: None, imports: None, fan_in: None, fan_out: None });
                for ii in &i.items {
                    if let ImplItem::Fn(m) = ii {
                        let metrics = compute_rust_function_metrics(&m.sig.inputs, &m.block, m.attrs.len());
                        units.push(UnitMetrics { file: file.to_string(), name: m.sig.ident.to_string(), kind: "method", line: m.sig.ident.span().start().line, statements: Some(metrics.statements), arguments: Some(metrics.arguments), indentation: Some(metrics.max_indentation), branches: Some(metrics.branches), returns: Some(metrics.returns), locals: Some(metrics.local_variables), methods: None, lines: None, imports: None, fan_in: None, fan_out: None });
                    }
                }
            }
            Item::Mod(m) => if let Some((_, items)) = &m.content { collect_detailed_from_items(items, file, units); }
            _ => {}
        }
    }
}

fn get_impl_name(i: &syn::ItemImpl) -> String {
    if let syn::Type::Path(tp) = &*i.self_ty {
        tp.path.segments.last().map_or_else(|| "<impl>".to_string(), |s| s.ident.to_string())
    } else {
        "<impl>".to_string()
    }
}

pub fn format_detailed_table(units: &[UnitMetrics]) -> String {
    let mut out = format!("{:<40} {:<20} {:<10} {:>5} {:>6} {:>5} {:>5} {:>5} {:>5} {:>6} {:>7} {:>5} {:>7} {:>6} {:>6}\n", "File", "Name", "Kind", "Line", "Stmts", "Args", "Ind", "Br", "Ret", "Locals", "Methods", "Lines", "Imports", "FanIn", "FanOut");
    out.push_str(&"-".repeat(152));
    out.push('\n');
    for u in units {
        let fmt = |v: Option<usize>| v.map_or_else(|| "-".to_string(), |n| n.to_string());
        out.push_str(&format!("{:<40} {:<20} {:<10} {:>5} {:>6} {:>5} {:>5} {:>5} {:>5} {:>6} {:>7} {:>5} {:>7} {:>6} {:>6}\n", truncate(&u.file, 40), truncate(&u.name, 20), u.kind, u.line, fmt(u.statements), fmt(u.arguments), fmt(u.indentation), fmt(u.branches), fmt(u.returns), fmt(u.locals), fmt(u.methods), fmt(u.lines), fmt(u.imports), fmt(u.fan_in), fmt(u.fan_out)));
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("...{}", &s[s.len() - max + 3..]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this_is_a_very_long_string", 10), "..._string");
    }

    #[test]
    fn test_format_detailed_table() {
        let units = vec![UnitMetrics {
            file: "test.rs".to_string(), name: "foo".to_string(), kind: "function",
            line: 1, statements: Some(5), arguments: Some(2), indentation: Some(1),
            branches: Some(0), returns: Some(1), locals: Some(3), methods: None,
            lines: None, imports: None, fan_in: None, fan_out: None,
        }];
        let table = format_detailed_table(&units);
        assert!(table.contains("test.rs"));
        assert!(table.contains("foo"));
    }

    #[test]
    fn test_get_impl_name() {
        let code: syn::ItemImpl = syn::parse_quote! { impl Foo { fn bar() {} } };
        assert_eq!(get_impl_name(&code), "Foo");
    }

    #[test]
    fn test_module_name_from_path() {
        use std::path::Path;
        assert_eq!(module_name_from_path(Path::new("src/foo.rs")), "foo");
        assert_eq!(module_name_from_path(Path::new("bar.py")), "bar");
    }

    #[test]
    fn test_collect_detailed_py_empty() {
        let units = collect_detailed_py(&[], None);
        assert!(units.is_empty());
    }

    #[test]
    fn test_collect_detailed_rs_empty() {
        let units = collect_detailed_rs(&[], None);
        assert!(units.is_empty());
    }

    #[test]
    fn test_collect_detailed_from_node() {
        use crate::parsing::{create_parser, parse_file};
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "def foo():\n    x = 1\nclass Bar:\n    def m(self): pass").unwrap();
        let parsed = parse_file(&mut create_parser().unwrap(), tmp.path()).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_node(parsed.tree.root_node(), &parsed.source, "t.py", &mut units);
        assert!(units.iter().any(|u| u.name == "foo" && u.kind == "function"));
        assert!(units.iter().any(|u| u.name == "Bar" && u.kind == "class"));
    }

    #[test]
    fn test_collect_detailed_from_items() {
        let code: syn::File = syn::parse_quote! { fn foo() { let x = 1; } impl Bar { fn m(&self) {} } };
        let mut units = Vec::new();
        collect_detailed_from_items(&code.items, "t.rs", &mut units);
        assert!(units.iter().any(|u| u.name == "foo" && u.kind == "function"));
        assert!(units.iter().any(|u| u.name == "Bar" && u.kind == "impl"));
    }
}
