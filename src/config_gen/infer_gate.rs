use std::path::Path;

use crate::code_roles::{RoleBuildError, SourceRoleIndex, is_test_only_file};
use crate::discovery::{Language, gather_files_by_lang};
use crate::duplication::{
    DuplicationConfig, cluster_duplicates, detect_duplicates_from_chunks,
    extract_chunks_for_duplication_with_roles, extract_rust_chunks_for_duplication_with_roles,
};
use crate::gate_config::GateConfig;
use crate::graph::{build_python_context_graph, collect_orphan_entry_paths, orphan_violations};
use crate::lang_analysis::parse_then_classify;
use crate::parsing::ParsedFile;
use crate::rust_graph::build_rust_context_graph;
use crate::rust_parsing::ParsedRustFile;

pub fn infer_gate_config_for_paths(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> Result<GateConfig, RoleBuildError> {
    let (py_files, rs_files) = gather_files_by_lang(paths, lang, ignore);
    let mut gate = GateConfig::default();

    let (py_parsed, rs_parsed, roles) = parse_then_classify(&py_files, &rs_files)?;
    let repo_root = paths
        .first()
        .map(Path::new)
        .unwrap_or_else(|| Path::new("."));
    gate.orphan_module_enabled =
        !has_orphan_modules(&py_parsed, &rs_parsed, Some(&roles), repo_root);

    let py_prod: Vec<ParsedFile> = py_parsed
        .into_iter()
        .filter(|p| !is_test_only_file(&roles, &p.path))
        .collect();
    let rs_prod: Vec<ParsedRustFile> = rs_parsed
        .into_iter()
        .filter(|p| !is_test_only_file(&roles, &p.path))
        .collect();

    gate.duplication_enabled =
        !has_reportable_duplicates(&py_prod, &rs_prod, Some(&roles), gate.min_similarity);
    gate.comment_removal_enabled =
        !crate::has_non_doc_comments_with_roles(&py_prod, &rs_prod, Some(&roles));
    Ok(gate)
}

pub(crate) fn has_orphan_modules(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    roles: Option<&SourceRoleIndex>,
    repo_root: &Path,
) -> bool {
    let empty_roles = SourceRoleIndex::empty();
    let roles = roles.unwrap_or(&empty_roles);
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let py_ctx = if py_parsed.is_empty() {
        crate::graph::ContextDependencyGraph::empty()
    } else {
        build_python_context_graph(&py_refs, roles)
    };
    let rs_ctx = if rs_parsed.is_empty() {
        crate::graph::ContextDependencyGraph::empty()
    } else {
        build_rust_context_graph(&rs_refs, roles)
    };
    let py_prod = py_ctx.production_view();
    let rs_prod = rs_ctx.production_view();
    let entries = collect_orphan_entry_paths(
        py_parsed,
        rs_parsed,
        (!py_parsed.is_empty()).then_some(&py_prod),
        (!rs_parsed.is_empty()).then_some(&rs_prod),
    );
    let has_orphan = |viols: &[crate::Violation]| viols.iter().any(|v| v.metric == "orphan_module");
    if !py_parsed.is_empty()
        && has_orphan(&orphan_violations(
            &py_ctx,
            &py_prod,
            &entries,
            &[],
            repo_root,
        ))
    {
        return true;
    }
    if !rs_parsed.is_empty()
        && has_orphan(&orphan_violations(
            &rs_ctx,
            &rs_prod,
            &entries,
            &[],
            repo_root,
        ))
    {
        return true;
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
