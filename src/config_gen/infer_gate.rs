use std::path::PathBuf;

use crate::cli_output::min_per_file_coverage;
use crate::config::Config;
use crate::discovery::{Language, gather_files_by_lang};
use crate::duplication::{
    DuplicationConfig, cluster_duplicates, detect_duplicates_from_chunks,
    extract_chunks_for_duplication, extract_rust_chunks_for_duplication,
};
use crate::gate_config::GateConfig;
use crate::graph::{analyze_graph, build_dependency_graph};
use crate::parsing::{ParsedFile, parse_files};
use crate::rust_graph::build_rust_dependency_graph;
use crate::rust_parsing::{ParsedRustFile, parse_rust_files};
use crate::rust_test_refs::analyze_rust_test_refs;
use crate::test_refs::analyze_test_refs;

type DefLineList = Vec<(PathBuf, String, usize)>;

pub fn infer_gate_config_for_paths(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> GateConfig {
    let (py_files, rs_files) = gather_files_by_lang(paths, lang, ignore);
    let mut gate = GateConfig::default();

    let py_parsed = if py_files.is_empty() {
        Vec::new()
    } else {
        parse_files(&py_files)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .collect::<Vec<ParsedFile>>()
    };
    let rs_parsed = if rs_files.is_empty() {
        Vec::new()
    } else {
        parse_rust_files(&rs_files)
            .into_iter()
            .filter_map(Result::ok)
            .collect::<Vec<ParsedRustFile>>()
    };

    gate.test_coverage_threshold = compute_min_per_file_test_coverage(&py_parsed, &rs_parsed);
    gate.duplication_enabled =
        !has_reportable_duplicates(&py_parsed, &rs_parsed, gate.min_similarity);
    gate.orphan_module_enabled = !has_orphan_modules(&py_parsed, &rs_parsed);
    gate
}

pub(crate) fn compute_min_per_file_test_coverage(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> usize {
    let (definitions, unreferenced) = collect_defs_and_unrefs(py_parsed, rs_parsed);
    min_per_file_coverage(&definitions, &unreferenced)
}

pub(super) fn extend_defs_from_py(
    definitions: &mut DefLineList,
    unreferenced: &mut DefLineList,
    py_parsed: &[ParsedFile],
) {
    if py_parsed.is_empty() {
        return;
    }
    let refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let a = analyze_test_refs(&refs, None);
    definitions.extend(
        a.definitions
            .iter()
            .map(|d| (d.file.clone(), d.name.clone(), d.line)),
    );
    unreferenced.extend(
        a.unreferenced
            .iter()
            .map(|d| (d.file.clone(), d.name.clone(), d.line)),
    );
}

pub(super) fn extend_defs_from_rs(
    definitions: &mut DefLineList,
    unreferenced: &mut DefLineList,
    rs_parsed: &[ParsedRustFile],
) {
    if rs_parsed.is_empty() {
        return;
    }
    let refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let a = analyze_rust_test_refs(&refs, None);
    definitions.extend(
        a.definitions
            .iter()
            .map(|d| (d.file.clone(), d.name.clone(), d.line)),
    );
    unreferenced.extend(
        a.unreferenced
            .iter()
            .map(|d| (d.file.clone(), d.name.clone(), d.line)),
    );
}

pub(super) fn collect_defs_and_unrefs(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> (DefLineList, DefLineList) {
    let mut definitions = DefLineList::new();
    let mut unreferenced = DefLineList::new();
    extend_defs_from_py(&mut definitions, &mut unreferenced, py_parsed);
    extend_defs_from_rs(&mut definitions, &mut unreferenced, rs_parsed);
    (definitions, unreferenced)
}

pub(crate) fn has_orphan_modules(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile]) -> bool {
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let has_orphan = |viols: &[crate::Violation]| viols.iter().any(|v| v.metric == "orphan_module");
    if !py_parsed.is_empty() {
        let graph = build_dependency_graph(&py_refs);
        if has_orphan(&analyze_graph(&graph, &py_config, true)) {
            return true;
        }
    }
    if !rs_parsed.is_empty() {
        let graph = build_rust_dependency_graph(&rs_refs);
        if has_orphan(&analyze_graph(&graph, &rs_config, true)) {
            return true;
        }
    }
    false
}

pub(crate) fn has_reportable_duplicates(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    min_similarity: f64,
) -> bool {
    let config = DuplicationConfig {
        min_similarity,
        ..Default::default()
    };
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let mut chunks = extract_chunks_for_duplication(&py_refs);
    chunks.extend(extract_rust_chunks_for_duplication(&rs_refs));
    if chunks.len() < 2 {
        return false;
    }
    let pairs = detect_duplicates_from_chunks(&chunks, &config);
    !cluster_duplicates(&pairs, &chunks).is_empty()
}
