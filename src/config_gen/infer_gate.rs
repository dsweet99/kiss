use crate::code_roles::{RoleBuildError, is_test_only_file};
use crate::config::Config;
use crate::discovery::{Language, gather_files_by_lang};
use crate::duplication::{
    DuplicationConfig, cluster_duplicates, detect_duplicates_from_chunks,
    extract_chunks_for_duplication_with_roles, extract_rust_chunks_for_duplication_with_roles,
};
use crate::gate_config::GateConfig;
use crate::graph::{analyze_graph, build_python_context_graph};
use crate::lang_analysis::parse_then_classify;
use crate::parsing::ParsedFile;
use crate::rust_graph::build_rust_dependency_graph_with_roles;
use crate::rust_parsing::ParsedRustFile;

pub fn infer_gate_config_for_paths(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> Result<GateConfig, RoleBuildError> {
    let (py_files, rs_files) = gather_files_by_lang(paths, lang, ignore);
    let mut gate = GateConfig::default();

    let (py_parsed, rs_parsed, roles) = parse_then_classify(&py_files, &rs_files)?;
    let py_parsed: Vec<ParsedFile> = py_parsed
        .into_iter()
        .filter(|p| !is_test_only_file(&roles, &p.path))
        .collect();
    let rs_parsed: Vec<ParsedRustFile> = rs_parsed
        .into_iter()
        .filter(|p| !is_test_only_file(&roles, &p.path))
        .collect();

    gate.duplication_enabled =
        !has_reportable_duplicates(&py_parsed, &rs_parsed, Some(&roles), gate.min_similarity);
    gate.orphan_module_enabled = !has_orphan_modules(&py_parsed, &rs_parsed, Some(&roles));
    gate.comment_removal_enabled =
        !crate::has_non_doc_comments_with_roles(&py_parsed, &rs_parsed, Some(&roles));
    Ok(gate)
}

pub(crate) fn has_orphan_modules(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    roles: Option<&crate::code_roles::SourceRoleIndex>,
) -> bool {
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let has_orphan = |viols: &[crate::Violation]| viols.iter().any(|v| v.metric == "orphan_module");
    if !py_parsed.is_empty() {
        let graph = match roles {
            Some(roles) => build_python_context_graph(&py_refs, roles).production_view(),
            None => crate::graph::build_dependency_graph(&py_refs),
        };
        if has_orphan(&analyze_graph(&graph, &py_config, true)) {
            return true;
        }
    }
    if !rs_parsed.is_empty() {
        let graph = build_rust_dependency_graph_with_roles(&rs_refs, roles);
        if has_orphan(&analyze_graph(&graph, &rs_config, true)) {
            return true;
        }
    }
    false
}

pub(crate) fn has_reportable_duplicates(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    roles: Option<&crate::code_roles::SourceRoleIndex>,
    min_similarity: f64,
) -> bool {
    let config = DuplicationConfig {
        min_similarity,
        ..Default::default()
    };
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let mut chunks = extract_chunks_for_duplication_with_roles(&py_refs, roles);
    chunks.extend(extract_rust_chunks_for_duplication_with_roles(
        &rs_refs, roles,
    ));
    if chunks.len() < 2 {
        return false;
    }
    let pairs = detect_duplicates_from_chunks(&chunks, &config);
    !cluster_duplicates(&pairs, &chunks).is_empty()
}
