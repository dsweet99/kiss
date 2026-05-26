//! Emit per-file static test-name-reference coverage as JSON on stdout.
//!
//! Usage:
//!   kiss-coverage-map REPO              # {"path": pct, ...}
//!   kiss-coverage-map --units REPO      # [{"file","name","line","kiss_covered"}, ...]
use kiss::cli_output::file_coverage_map;
use kiss::discovery::gather_files_by_lang;
use kiss::graph::build_dependency_graph;
use kiss::rust_graph::build_rust_dependency_graph;
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

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let units_mode = raw.first().is_some_and(|a| a == "--units");
    let path = if units_mode {
        raw.get(1).cloned().unwrap_or_else(|| ".".into())
    } else {
        raw.first().cloned().unwrap_or_else(|| ".".into())
    };
    let paths = vec![path];
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
    let py_graph = if py_parsed.is_empty() {
        None
    } else {
        Some(build_dependency_graph(&py_refs))
    };
    let rs_graph = if rs_parsed.is_empty() {
        None
    } else {
        Some(build_rust_dependency_graph(&rs_refs))
    };
    let py_cov = kiss::analyze_test_refs(&py_refs, py_graph.as_ref());
    let rs_cov = kiss::analyze_rust_test_refs(&rs_refs, rs_graph.as_ref());

    if units_mode {
        let mut unref: HashSet<(String, String)> = HashSet::new();
        for d in &py_cov.unreferenced {
            unref.insert((d.file.to_string_lossy().into_owned(), d.name.clone()));
        }
        for d in &rs_cov.unreferenced {
            unref.insert((d.file.to_string_lossy().into_owned(), d.name.clone()));
        }
        let mut rows: Vec<UnitRow> = Vec::new();
        for d in &py_cov.definitions {
            let file = d.file.to_string_lossy().into_owned();
            let kiss_covered = !unref.contains(&(file.clone(), d.name.clone()));
            rows.push(UnitRow {
                file,
                name: d.name.clone(),
                line: d.line,
                kiss_covered,
            });
        }
        for d in &rs_cov.definitions {
            let file = d.file.to_string_lossy().into_owned();
            let kiss_covered = !unref.contains(&(file.clone(), d.name.clone()));
            rows.push(UnitRow {
                file,
                name: d.name.clone(),
                line: d.line,
                kiss_covered,
            });
        }
        println!("{}", serde_json::to_string(&rows).expect("json"));
        return;
    }

    let py_defs: Vec<(PathBuf, String, usize)> = py_cov
        .definitions
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let py_unref: Vec<(PathBuf, String, usize)> = py_cov
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let rs_defs: Vec<(PathBuf, String, usize)> = rs_cov
        .definitions
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let rs_unref: Vec<(PathBuf, String, usize)> = rs_cov
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();

    let mut map = file_coverage_map(&py_defs, &py_unref);
    for (file, pct) in file_coverage_map(&rs_defs, &rs_unref) {
        map.insert(file, pct);
    }

    let out: HashMap<String, usize> = map
        .into_iter()
        .map(|(p, pct)| (p.to_string_lossy().into_owned(), pct))
        .collect();
    println!("{}", serde_json::to_string(&out).expect("json"));
}
