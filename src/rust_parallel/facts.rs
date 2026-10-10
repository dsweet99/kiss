use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::code_roles::{SourceRoleIndex, is_test_only_file, production_line_count};
use crate::compute_rust_file_metrics_with_roles;
use crate::config::Config;
use crate::duplication::{
    CodeChunk, DuplicationConfig, cluster_duplicates_from_chunks,
    extract_rust_chunks_for_duplication_with_roles,
};
use crate::gate_config::GateConfig;
use crate::graph::DependencyGraph;
use crate::rust_counts::{analyze_rust_file_with_roles, file_threshold_violations};
use crate::rust_graph::{RustImports, context_graph_from_imports, include_graph_from_literals};
use crate::rust_include::canonical_path;
use crate::rust_parsing::ParsedRustFile;
use crate::rust_units::extract_rust_code_units;
use crate::violation::Violation;
use crate::{RoleBuildError, RustFileMetrics};

use super::ParallelRustOutput;
use super::pool::Pool;

pub(super) struct FileFacts {
    units: usize,
    stmts: usize,
    viols: Vec<Violation>,
    metrics: RustFileMetrics,
    lines: usize,
    imports: RustImports,
    chunks: Vec<CodeChunk>,
    comments: Vec<Violation>,
    docs: Vec<Violation>,
}

struct ScanFlags {
    comments: bool,
    dups: bool,
}

pub(super) fn map_facts(
    pool: &Pool,
    roles: &SourceRoleIndex,
    config: &Config,
    gate: &GateConfig,
    repo_root: &Path,
) -> Result<Vec<FileFacts>, RoleBuildError> {
    let roles = Arc::new(roles.clone());
    let config = Arc::new(config.clone());
    let docs_allowed = gate.docs_allowed.clone();
    let repo_root = repo_root.to_path_buf();
    let flags = ScanFlags {
        comments: gate.comment_removal_enabled,
        dups: gate.duplication_enabled,
    };
    pool.map(move |parsed| one_file(parsed, &roles, &config, &flags, &docs_allowed, &repo_root))
}

fn one_file(
    parsed: &ParsedRustFile,
    roles: &SourceRoleIndex,
    config: &Config,
    flags: &ScanFlags,
    docs_allowed: &[String],
    repo_root: &Path,
) -> FileFacts {
    let test_only = is_test_only_file(roles, &parsed.path);
    let metrics = compute_rust_file_metrics_with_roles(parsed, Some(roles));
    let lines = production_line_count(roles, &parsed.path, &parsed.source);
    let (units, stmts, viols) = if test_only {
        (0, 0, Vec::new())
    } else {
        (
            extract_rust_code_units(parsed).len(),
            metrics.statements,
            analyze_rust_file_with_roles(parsed, config, Some(roles)),
        )
    };
    let imports = crate::rust_graph::extract_rust_imports(&parsed.ast);
    let chunks = if flags.dups && !test_only {
        extract_rust_chunks_for_duplication_with_roles(std::slice::from_ref(&parsed), Some(roles))
    } else {
        Vec::new()
    };
    let comments = if flags.comments {
        crate::comments::rust_file_comment_violations(parsed, Some(roles))
    } else {
        Vec::new()
    };
    let docs =
        crate::comments::rust_file_doc_violations(parsed, Some(roles), docs_allowed, repo_root);
    FileFacts {
        units,
        stmts,
        viols,
        metrics,
        lines,
        imports,
        chunks,
        comments,
        docs,
    }
}

pub(super) fn assemble_output(
    files: &[PathBuf],
    roles: SourceRoleIndex,
    mut facts: Vec<FileFacts>,
    sources: Vec<String>,
    config: &Config,
    min_similarity: f64,
) -> ParallelRustOutput {
    let rollups = include_rollups(files, &facts, &roles, config);
    let graph = rust_graph(files, &facts, &roles);
    let mut units = 0;
    let mut stmts = 0;
    let mut viols = Vec::new();
    let mut comments = Vec::new();
    let mut docs = Vec::new();
    let mut chunks = Vec::new();
    for fact in &mut facts {
        units += fact.units;
        stmts += fact.stmts;
        viols.append(&mut fact.viols);
        comments.append(&mut fact.comments);
        docs.append(&mut fact.docs);
        chunks.append(&mut fact.chunks);
    }
    viols.extend(rollups);
    let dups = cluster_duplicates_from_chunks(
        &chunks,
        &DuplicationConfig {
            min_similarity,
            ..DuplicationConfig::default()
        },
    );
    ParallelRustOutput {
        sources: files.iter().cloned().zip(sources).collect(),
        roles,
        units,
        stmts,
        viols,
        comments,
        docs,
        graph,
        dups,
    }
}

fn include_rollups(
    files: &[PathBuf],
    facts: &[FileFacts],
    roles: &SourceRoleIndex,
    config: &Config,
) -> Vec<Violation> {
    let rows: Vec<(PathBuf, Vec<String>)> = files
        .iter()
        .zip(facts)
        .map(|(path, fact)| (path.clone(), fact.imports.include_literals.clone()))
        .collect();
    let include_graph = include_graph_from_literals(&rows);
    let mut by_path = HashMap::new();
    for (idx, path) in files.iter().enumerate() {
        by_path.insert(canonical_path(path), idx);
    }
    let mut viols = Vec::new();
    for (idx, path) in files.iter().enumerate() {
        if is_test_only_file(roles, path) {
            continue;
        }
        let included = included_indexes(path, &include_graph, &by_path);
        if included.is_empty() {
            continue;
        }
        viols.extend(rollup_one(
            path,
            files,
            &facts[idx],
            &included,
            facts,
            config,
        ));
    }
    viols
}

fn included_indexes(
    path: &Path,
    include_graph: &crate::rust_graph::IncludeGraph,
    by_path: &HashMap<PathBuf, usize>,
) -> Vec<usize> {
    include_graph
        .transitive_from(path)
        .iter()
        .filter_map(|path| by_path.get(path).copied())
        .collect()
}

fn rollup_one(
    path: &Path,
    files: &[PathBuf],
    parent: &FileFacts,
    included: &[usize],
    facts: &[FileFacts],
    config: &Config,
) -> Vec<Violation> {
    let mut merged = parent.metrics.clone();
    let mut lines = parent.lines;
    let mut contributors = Vec::new();
    for idx in included {
        let frag = &facts[*idx];
        merged.statements += frag.metrics.statements;
        merged.interface_types += frag.metrics.interface_types;
        merged.concrete_types += frag.metrics.concrete_types;
        merged.imports += frag.metrics.imports;
        merged.functions += frag.metrics.functions;
        lines += frag.lines;
        contributors.push(files[*idx].display().to_string());
    }
    file_threshold_violations(path, config, &merged, lines, &contributors.join(", "))
}

fn rust_graph(
    files: &[PathBuf],
    facts: &[FileFacts],
    roles: &SourceRoleIndex,
) -> Option<DependencyGraph> {
    if files.is_empty() {
        return None;
    }
    let owned: Vec<(PathBuf, RustImports)> = files
        .iter()
        .zip(facts)
        .map(|(path, fact)| (path.clone(), fact.imports.clone()))
        .collect();
    Some(context_graph_from_imports(&owned, roles).production_view())
}
