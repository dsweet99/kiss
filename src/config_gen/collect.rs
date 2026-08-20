use std::path::Path;

use crate::discovery::{Language, gather_files_by_lang};
use crate::graph::{GraphKeyMaxima, build_dependency_graph, graph_key_maxima};
use crate::parsing::{ParsedFile, parse_files};
use crate::rust_graph::build_rust_dependency_graph;
use crate::rust_parsing::{ParsedRustFile, parse_rust_files};
use crate::stats::MetricStats;

pub struct CollectedLang {
    pub stats: MetricStats,
    pub file_count: usize,
    pub graph_max: GraphKeyMaxima,
}

pub fn collect_py_stats(root: &Path) -> (MetricStats, usize) {
    collect_py_stats_with_ignore(root, &[])
}

pub fn collect_py_stats_with_ignore(root: &Path, ignore: &[String]) -> (MetricStats, usize) {
    let collected = collect_lang_from_paths(
        &[root.to_string_lossy().into()],
        Some(Language::Python),
        ignore,
    )
    .0;
    (collected.stats, collected.file_count)
}

pub fn collect_rs_stats(root: &Path) -> (MetricStats, usize) {
    collect_rs_stats_with_ignore(root, &[])
}

pub fn collect_rs_stats_with_ignore(root: &Path, ignore: &[String]) -> (MetricStats, usize) {
    let collected = collect_lang_from_paths(
        &[root.to_string_lossy().into()],
        Some(Language::Rust),
        ignore,
    )
    .1;
    (collected.stats, collected.file_count)
}

pub fn collect_all_stats(
    paths: &[String],
    lang: Option<Language>,
) -> ((MetricStats, usize), (MetricStats, usize)) {
    collect_all_stats_with_ignore(paths, lang, &[])
}

pub fn collect_all_stats_with_ignore(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> ((MetricStats, usize), (MetricStats, usize)) {
    let (py, rs) = collect_lang_from_paths(paths, lang, ignore);
    ((py.stats, py.file_count), (rs.stats, rs.file_count))
}

pub fn collect_lang_from_paths(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> (CollectedLang, CollectedLang) {
    let (py_files, rs_files) = gather_files_by_lang(paths, lang, ignore);
    (
        collect_python_from_files(&py_files),
        collect_rust_from_files(&rs_files),
    )
}

fn keep_parsed_python(
    result: Result<ParsedFile, crate::parsing::ParseError>,
) -> Option<ParsedFile> {
    match result {
        Ok(parsed) => Some(parsed),
        Err(err) => {
            eprintln!("Error parsing Python: {err}");
            None
        }
    }
}

fn keep_parsed_rust(
    result: Result<ParsedRustFile, crate::rust_parsing::RustParseError>,
) -> Option<ParsedRustFile> {
    match result {
        Ok(parsed) => Some(parsed),
        Err(err) => {
            eprintln!("Error parsing Rust: {err}");
            None
        }
    }
}

fn collect_python_from_files(py_files: &[std::path::PathBuf]) -> CollectedLang {
    if py_files.is_empty() {
        return CollectedLang {
            stats: MetricStats::default(),
            file_count: 0,
            graph_max: GraphKeyMaxima::default(),
        };
    }
    let results = match parse_files(py_files) {
        Ok(results) => results,
        Err(err) => {
            eprintln!("Failed to initialize Python parser: {err}");
            return CollectedLang {
                stats: MetricStats::default(),
                file_count: py_files.len(),
                graph_max: GraphKeyMaxima::default(),
            };
        }
    };
    let parsed: Vec<ParsedFile> = results.into_iter().filter_map(keep_parsed_python).collect();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let stats = if refs.is_empty() {
        MetricStats::default()
    } else {
        MetricStats::collect(&refs)
    };
    let graph_max = if refs.is_empty() {
        GraphKeyMaxima::default()
    } else {
        graph_key_maxima(&build_dependency_graph(&refs))
    };
    CollectedLang {
        stats,
        file_count: py_files.len(),
        graph_max,
    }
}

fn collect_rust_from_files(rs_files: &[std::path::PathBuf]) -> CollectedLang {
    if rs_files.is_empty() {
        return CollectedLang {
            stats: MetricStats::default(),
            file_count: 0,
            graph_max: GraphKeyMaxima::default(),
        };
    }
    let parsed: Vec<ParsedRustFile> = parse_rust_files(rs_files)
        .into_iter()
        .filter_map(keep_parsed_rust)
        .collect();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let stats = if refs.is_empty() {
        MetricStats::default()
    } else {
        MetricStats::collect_rust(&refs)
    };
    let graph_max = if refs.is_empty() {
        GraphKeyMaxima::default()
    } else {
        graph_key_maxima(&build_rust_dependency_graph(&refs))
    };
    CollectedLang {
        stats,
        file_count: rs_files.len(),
        graph_max,
    }
}
