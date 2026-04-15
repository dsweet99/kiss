use std::path::Path;

use crate::discovery::{Language, find_source_files_with_ignore};
use crate::graph::build_dependency_graph;
use crate::parsing::{ParsedFile, parse_files};
use crate::rust_graph::build_rust_dependency_graph;
use crate::rust_parsing::{ParsedRustFile, parse_rust_files};
use crate::stats::MetricStats;

pub fn collect_py_stats(root: &Path) -> (MetricStats, usize) {
    collect_py_stats_with_ignore(root, &[])
}

pub fn collect_py_stats_with_ignore(root: &Path, ignore: &[String]) -> (MetricStats, usize) {
    let py_files: Vec<_> = find_source_files_with_ignore(root, ignore)
        .into_iter()
        .filter(|sf| sf.language == Language::Python)
        .map(|sf| sf.path)
        .collect();
    if py_files.is_empty() {
        return (MetricStats::default(), 0);
    }
    let Ok(results) = parse_files(&py_files) else {
        return (MetricStats::default(), 0);
    };
    let parsed: Vec<ParsedFile> = results
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect();
    let cnt = parsed.len();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let mut stats = MetricStats::collect(&refs);
    stats.collect_graph_metrics(&build_dependency_graph(&refs));
    (stats, cnt)
}

pub fn collect_rs_stats(root: &Path) -> (MetricStats, usize) {
    collect_rs_stats_with_ignore(root, &[])
}

pub fn collect_rs_stats_with_ignore(root: &Path, ignore: &[String]) -> (MetricStats, usize) {
    let rs_files: Vec<_> = find_source_files_with_ignore(root, ignore)
        .into_iter()
        .filter(|sf| sf.language == Language::Rust)
        .map(|sf| sf.path)
        .collect();
    if rs_files.is_empty() {
        return (MetricStats::default(), 0);
    }
    let parsed: Vec<ParsedRustFile> = parse_rust_files(&rs_files)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect();
    let cnt = parsed.len();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let mut stats = MetricStats::collect_rust(&refs);
    stats.collect_graph_metrics(&build_rust_dependency_graph(&refs));
    (stats, cnt)
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
    let (mut py, mut rs) = ((MetricStats::default(), 0), (MetricStats::default(), 0));
    for path in paths {
        let root = Path::new(path);
        if lang.is_none() || lang == Some(Language::Python) {
            let (s, c) = collect_py_stats_with_ignore(root, ignore);
            py.0.merge(s);
            py.1 += c;
        }
        if lang.is_none() || lang == Some(Language::Rust) {
            let (s, c) = collect_rs_stats_with_ignore(root, ignore);
            rs.0.merge(s);
            rs.1 += c;
        }
    }
    (py, rs)
}
