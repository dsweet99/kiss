use std::collections::HashMap;
use std::path::Path;

use crate::code_roles::{
    RoleBuildError, SourceRoleIndex, is_test_only_file, production_line_count,
};
use crate::discovery::{Language, gather_files_by_lang};
use crate::graph::{GraphKeyMaxima, graph_key_maxima};
use crate::lang_analysis::parse_then_classify;
use crate::parsing::ParsedFile;
use crate::rust_fn_metrics::compute_rust_file_metrics_with_roles;
use crate::rust_graph::{build_include_graph, build_rust_dependency_graph_with_roles};
use crate::rust_include::canonical_path;
use crate::rust_parsing::ParsedRustFile;
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
    match collect_lang_from_paths(
        &[root.to_string_lossy().into()],
        Some(Language::Python),
        ignore,
    ) {
        Ok((py, _)) => (py.stats, py.file_count),
        Err(err) => {
            eprintln!("{err}");
            (MetricStats::default(), 0)
        }
    }
}

pub fn collect_rs_stats(root: &Path) -> (MetricStats, usize) {
    collect_rs_stats_with_ignore(root, &[])
}

pub fn collect_rs_stats_with_ignore(root: &Path, ignore: &[String]) -> (MetricStats, usize) {
    match collect_lang_from_paths(
        &[root.to_string_lossy().into()],
        Some(Language::Rust),
        ignore,
    ) {
        Ok((_, rs)) => (rs.stats, rs.file_count),
        Err(err) => {
            eprintln!("{err}");
            (MetricStats::default(), 0)
        }
    }
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
    match collect_lang_from_paths(paths, lang, ignore) {
        Ok((py, rs)) => ((py.stats, py.file_count), (rs.stats, rs.file_count)),
        Err(err) => {
            eprintln!("{err}");
            ((MetricStats::default(), 0), (MetricStats::default(), 0))
        }
    }
}

pub fn collect_lang_from_paths(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> Result<(CollectedLang, CollectedLang), RoleBuildError> {
    let (py_files, rs_files) = gather_files_by_lang(paths, lang, ignore);
    let (py_parsed, rs_parsed, roles) = parse_then_classify(&py_files, &rs_files)?;
    Ok((
        collect_python_from_parsed(&py_parsed, &roles),
        collect_rust_from_parsed(&rs_parsed, &roles),
    ))
}

fn collect_python_from_parsed(parsed: &[ParsedFile], roles: &SourceRoleIndex) -> CollectedLang {
    let refs: Vec<&ParsedFile> = parsed
        .iter()
        .filter(|p| !is_test_only_file(roles, &p.path))
        .collect();
    if refs.is_empty() {
        return CollectedLang {
            stats: MetricStats::default(),
            file_count: 0,
            graph_max: GraphKeyMaxima::default(),
        };
    }
    CollectedLang {
        stats: MetricStats::collect(&refs),
        file_count: refs.len(),
        graph_max: graph_key_maxima(
            &crate::graph::build_python_context_graph(&refs, roles).production_view(),
        ),
    }
}

fn collect_rust_from_parsed(parsed: &[ParsedRustFile], roles: &SourceRoleIndex) -> CollectedLang {
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let production = refs
        .iter()
        .filter(|p| !is_test_only_file(roles, &p.path))
        .count();
    if refs.is_empty() {
        return CollectedLang {
            stats: MetricStats::default(),
            file_count: 0,
            graph_max: GraphKeyMaxima::default(),
        };
    }
    let mut stats = MetricStats::collect_rust_with_roles(&refs, Some(roles));
    push_include_rollup_file_metrics(&mut stats, &refs, roles);
    CollectedLang {
        stats,
        file_count: production,
        graph_max: graph_key_maxima(&build_rust_dependency_graph_with_roles(&refs, Some(roles))),
    }
}

fn push_include_rollup_file_metrics(
    stats: &mut MetricStats,
    parsed: &[&ParsedRustFile],
    roles: &SourceRoleIndex,
) {
    let include_graph = build_include_graph(parsed);
    let by_path: HashMap<_, _> = parsed
        .iter()
        .map(|file| (canonical_path(&file.path), *file))
        .collect();
    for parent in parsed {
        if is_test_only_file(roles, &parent.path) {
            continue;
        }
        let included_paths = include_graph.transitive_from(&parent.path);
        let included: Vec<_> = included_paths
            .iter()
            .filter_map(|path| by_path.get(path).copied())
            .collect();
        if included.is_empty() {
            continue;
        }
        let mut merged = compute_rust_file_metrics_with_roles(parent, Some(roles));
        let mut lines = production_line_count(roles, &parent.path, &parent.source);
        for frag in included {
            let metrics = compute_rust_file_metrics_with_roles(frag, Some(roles));
            merged.statements += metrics.statements;
            merged.interface_types += metrics.interface_types;
            merged.concrete_types += metrics.concrete_types;
            merged.imports += metrics.imports;
            merged.functions += metrics.functions;
            lines += production_line_count(roles, &frag.path, &frag.source);
        }
        stats.statements_per_file.push(merged.statements);
        stats.interface_types_per_file.push(merged.interface_types);
        stats.concrete_types_per_file.push(merged.concrete_types);
        stats.imported_names_per_file.push(merged.imports);
        stats.functions_per_file.push(merged.functions);
        stats.lines_per_file.push(lines);
    }
}
