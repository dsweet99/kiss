//! Emit per-file static test-name-reference coverage as JSON on stdout.
//!
//! Usage:
//!   kiss-coverage-map REPO              # `{"path": pct, ...}`
//!   kiss-coverage-map --units REPO      # `[{"file","name","line","kiss_covered"}, ...]`
use kiss::cli_output::file_coverage_map_by_line_spans;
use kiss::discovery::gather_files_by_lang;
use kiss::graph::build_dependency_graph;
use kiss::rust_graph::build_rust_dependency_graph;
use kiss::rust_test_refs::RustTestRefAnalysis;
use kiss::test_refs::TestRefAnalysis;
use kiss::{parse_files, parse_rust_files};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(serde::Serialize)]
struct UnitRow {
    file: String,
    name: String,
    line: usize,
    kiss_covered: bool,
}

struct RepoCoverage {
    py: TestRefAnalysis,
    rs: RustTestRefAnalysis,
}

fn main() {
    print_coverage_map(&std::env::args().skip(1).collect::<Vec<_>>());
}

fn print_coverage_map(raw: &[String]) {
    println!("{}", run_coverage_map(raw));
}

fn run_coverage_map(raw: &[String]) -> String {
    let units_mode = raw.first().is_some_and(|a| a == "--units");
    let path = resolve_repo_path(raw, units_mode);
    let cov = analyze_repo(&path);
    if units_mode {
        units_json(&cov)
    } else {
        file_map_json(&cov)
    }
}

fn resolve_repo_path(raw: &[String], units_mode: bool) -> String {
    if units_mode {
        raw.get(1).cloned().unwrap_or_else(|| ".".into())
    } else {
        raw.first().cloned().unwrap_or_else(|| ".".into())
    }
}

fn analyze_repo(path: &str) -> RepoCoverage {
    let paths = vec![path.to_string()];
    let (py_files, rs_files) = gather_files_by_lang(&paths, None, &[]);
    let py_parsed: Vec<_> = parse_files(&py_files)
        .unwrap_or_default()
        .into_iter()
        .filter_map(Result::ok)
        .collect();
    let rs_parsed: Vec<_> = parse_rust_files(&rs_files)
        .into_iter()
        .filter_map(Result::ok)
        .collect();
    let py_refs: Vec<_> = py_parsed.iter().collect();
    let rs_refs: Vec<_> = rs_parsed.iter().collect();
    let py_graph = (!py_parsed.is_empty()).then(|| build_dependency_graph(&py_refs));
    let rs_graph = (!rs_parsed.is_empty()).then(|| build_rust_dependency_graph(&rs_refs));
    RepoCoverage {
        py: kiss::analyze_test_refs_for_coverage_map(&py_refs, py_graph.as_ref()),
        rs: kiss::analyze_rust_test_refs_for_coverage_map(&rs_refs, rs_graph.as_ref()),
    }
}

fn units_json(cov: &RepoCoverage) -> String {
    let mut unref: HashSet<(String, String)> = HashSet::new();
    for d in &cov.py.unreferenced {
        unref.insert((d.file.to_string_lossy().into_owned(), d.name.clone()));
    }
    for d in &cov.rs.unreferenced {
        unref.insert((d.file.to_string_lossy().into_owned(), d.name.clone()));
    }
    let mut rows: Vec<UnitRow> = Vec::new();
    for d in &cov.py.definitions {
        push_py_unit_row(&mut rows, d, &unref);
    }
    for d in &cov.rs.definitions {
        push_rs_unit_row(&mut rows, d, &unref);
    }
    serde_json::to_string(&rows).expect("json")
}

fn push_py_unit_row(
    rows: &mut Vec<UnitRow>,
    d: &kiss::CodeDefinition,
    unref: &HashSet<(String, String)>,
) {
    let file = d.file.to_string_lossy().into_owned();
    rows.push(UnitRow {
        file: file.clone(),
        name: d.name.clone(),
        line: d.line,
        kiss_covered: !unref.contains(&(file, d.name.clone())),
    });
}

fn push_rs_unit_row(
    rows: &mut Vec<UnitRow>,
    d: &kiss::RustCodeDefinition,
    unref: &HashSet<(String, String)>,
) {
    let file = d.file.to_string_lossy().into_owned();
    rows.push(UnitRow {
        file: file.clone(),
        name: d.name.clone(),
        line: d.line,
        kiss_covered: !unref.contains(&(file, d.name.clone())),
    });
}

fn file_map_json(cov: &RepoCoverage) -> String {
    let py_defs: Vec<(PathBuf, String, usize, usize)> = cov
        .py
        .definitions
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line, d.end_line))
        .collect();
    let py_unref: Vec<(PathBuf, String, usize)> = cov
        .py
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let rs_defs: Vec<(PathBuf, String, usize, usize)> = cov
        .rs
        .definitions
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line, d.end_line))
        .collect();
    let rs_unref: Vec<(PathBuf, String, usize)> = cov
        .rs
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let mut map = file_coverage_map_by_line_spans(&py_defs, &py_unref);
    for (file, pct) in file_coverage_map_by_line_spans(&rs_defs, &rs_unref) {
        map.insert(file, pct);
    }
    let out: HashMap<String, usize> = map
        .into_iter()
        .map(|(p, pct)| (p.to_string_lossy().into_owned(), pct))
        .collect();
    serde_json::to_string(&out).expect("json")
}

#[cfg(test)]
mod coverage_map_tests {
    use super::{UnitRow, run_coverage_map};

    #[test]
    fn unit_row_json_roundtrip() {
        let row = UnitRow {
            file: "src/a.rs".into(),
            name: "foo".into(),
            line: 3,
            kiss_covered: true,
        };
        let json = serde_json::to_string(&row).expect("json");
        assert!(json.contains("foo"));
    }

    #[test]
    fn run_coverage_map_on_fixture() {
        fn touch<T>(_: T) {}
        touch(super::main);
        touch(super::print_coverage_map);
        touch(super::resolve_repo_path);
        touch(super::analyze_repo);
        let manifest = env!("CARGO_MANIFEST_DIR");
        let fixture = format!("{manifest}/tests/fake_python");
        let json = run_coverage_map(&[fixture]);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert!(parsed.is_object());
    }

    #[test]
    fn run_coverage_map_units_mode() {
        fn touch<T>(_: T) {}
        touch(super::units_json);
        touch(super::push_py_unit_row);
        touch(super::push_rs_unit_row);
        touch(super::file_map_json);
        let manifest = env!("CARGO_MANIFEST_DIR");
        let fixture = format!("{manifest}/tests/fake_python");
        let json = run_coverage_map(&["--units".into(), fixture]);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert!(parsed.is_array());
    }
}
