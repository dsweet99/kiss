use crate::analyze::dup_detect::detect_py_duplicates;
use crate::analyze::graph_api::build_py_graphs;
use crate::analyze::options::AnalyzeOptions;
use crate::analyze::parallel::RustAnalysis;
use crate::analyze_parse::analyze_py_parsed;
use kiss::code_roles::SourceRoleIndex;
use kiss::{DependencyGraph, DuplicateCluster, ParsedFile, ParsedRustFile, Violation};
use std::path::PathBuf;

pub(crate) struct PySide {
    pub roles: SourceRoleIndex,
    pub units: usize,
    pub stmts: usize,
    pub viols: Vec<Violation>,
    pub comments: Vec<Violation>,
    pub graph: Option<DependencyGraph>,
    pub dups: Vec<DuplicateCluster>,
}

pub(crate) struct RsSide {
    pub parsed: Vec<ParsedRustFile>,
    pub roles: SourceRoleIndex,
    pub units: usize,
    pub stmts: usize,
    pub viols: Vec<Violation>,
    pub comments: Vec<Violation>,
    pub docs: Vec<Violation>,
    pub analysis: RustAnalysis,
}

pub(crate) fn parse_and_split_sides(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    opts: &AnalyzeOptions<'_>,
) -> Result<(Vec<ParsedFile>, PySide, RsSide), kiss::RoleBuildError> {
    let (py_parsed, py_side, rs_side) = std::thread::scope(|scope| {
        let rs_handle = scope.spawn(|| run_rust_after_parse(rs_files, opts));
        let py_parsed = crate::analyze_parse::parse_py_files_pooled(py_files)?;
        let py_side = run_python_side(&py_parsed, py_files, opts);
        let rs_out = rs_handle.join().expect("rust analysis thread");
        match (py_side, rs_out) {
            (Ok(py_side), Ok(rs_out)) => Ok((py_parsed, py_side, rs_side_from(rs_out))),
            (_, Err(err)) | (Err(err), _) => Err(err),
        }
    })?;
    Ok((py_parsed, py_side, rs_side))
}

fn rs_side_from(out: kiss::ParallelRustOutput) -> RsSide {
    let ast = syn::parse_file("").unwrap_or_else(|_| syn::parse_file("fn _kiss() {}").unwrap());
    let parsed = out
        .sources
        .into_iter()
        .map(|(path, source)| ParsedRustFile {
            path,
            source,
            ast: ast.clone(),
        })
        .collect();
    RsSide {
        parsed,
        roles: out.roles,
        units: out.units,
        stmts: out.stmts,
        viols: out.viols,
        comments: out.comments,
        docs: out.docs,
        analysis: RustAnalysis {
            graph: out.graph,
            dups: out.dups,
        },
    }
}

pub(crate) fn run_python_side(
    parsed: &[ParsedFile],
    py_files: &[PathBuf],
    opts: &AnalyzeOptions<'_>,
) -> Result<PySide, kiss::RoleBuildError> {
    let t0 = std::time::Instant::now();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let roles = kiss::code_roles::classify_python(&refs, py_files)?;
    let t1 = std::time::Instant::now();
    let ((units, stmts, viols), (comments, graph, dups)) = rayon::join(
        || analyze_py_parsed(parsed, opts.py_config, &roles),
        || py_side_rest(parsed, opts, &roles),
    );
    let t2 = std::time::Instant::now();
    if opts.show_timing {
        eprintln!(
            "[TIMING] py_side roles={:.2}s rest={:.2}s",
            t1.duration_since(t0).as_secs_f64(),
            t2.duration_since(t1).as_secs_f64()
        );
    }
    Ok(PySide {
        comments,
        graph,
        dups,
        roles,
        units,
        stmts,
        viols,
    })
}

pub(crate) fn run_rust_after_parse(
    rs_files: &[PathBuf],
    opts: &AnalyzeOptions<'_>,
) -> Result<kiss::ParallelRustOutput, kiss::RoleBuildError> {
    kiss::parallel_rust_analysis(kiss::ParallelRustRequest {
        files: rs_files,
        config: opts.rs_config,
        gate: opts.gate_config,
        repo_root: &crate::analyze_cache::repo_root_for_universe(opts.universe),
        show_timing: opts.show_timing,
    })
}

fn py_graph_and_dups(
    parsed: &[ParsedFile],
    opts: &AnalyzeOptions<'_>,
    roles: &SourceRoleIndex,
) -> (Option<DependencyGraph>, Vec<DuplicateCluster>) {
    let ((graph, _ctx), dups) = rayon::join(
        || build_py_graphs(parsed, roles),
        || py_dups(parsed, opts, roles),
    );
    (graph, dups)
}

fn py_side_rest(
    parsed: &[ParsedFile],
    opts: &AnalyzeOptions<'_>,
    roles: &SourceRoleIndex,
) -> (
    Vec<Violation>,
    Option<DependencyGraph>,
    Vec<DuplicateCluster>,
) {
    let (comments, (graph, dups)) = rayon::join(
        || py_comments(parsed, opts, roles),
        || py_graph_and_dups(parsed, opts, roles),
    );
    (comments, graph, dups)
}

fn py_comments(
    parsed: &[ParsedFile],
    opts: &AnalyzeOptions<'_>,
    roles: &SourceRoleIndex,
) -> Vec<Violation> {
    if opts.gate_config.comment_removal_enabled {
        kiss::collect_comment_violations_with_roles(parsed, &[], Some(roles))
    } else {
        Vec::new()
    }
}

fn py_dups(
    parsed: &[ParsedFile],
    opts: &AnalyzeOptions<'_>,
    roles: &SourceRoleIndex,
) -> Vec<DuplicateCluster> {
    if opts.gate_config.duplication_enabled {
        detect_py_duplicates(parsed, opts.gate_config.min_similarity, roles)
    } else {
        Vec::new()
    }
}
