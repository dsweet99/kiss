use crate::analyze::dup_detect::{detect_py_duplicates, detect_rs_duplicates};
use crate::analyze::graph_api::{build_py_graphs, build_rs_graphs};
use crate::analyze::options::AnalyzeOptions;
use crate::analyze::parallel::RustAnalysis;
use crate::analyze_parse::{analyze_py_parsed, analyze_rs_parsed};
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
    pub analysis: RustAnalysis,
}

pub(crate) fn parse_and_split_sides(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    opts: &AnalyzeOptions<'_>,
) -> Result<(Vec<ParsedFile>, PySide, RsSide), kiss::RoleBuildError> {
    let rs_parsed = crate::analyze_parse::parse_rs_files(rs_files)?;
    let py_parsed = crate::analyze_parse::parse_py_files_pooled(py_files)?;
    let (py_side, rs_side) = std::thread::scope(|scope| {
        let py_handle = scope.spawn(|| run_python_side(&py_parsed, py_files, opts));
        let rs_side = run_rust_after_parse(rs_parsed, rs_files, opts);
        (py_handle.join().expect("python analysis thread"), rs_side)
    });
    match (py_side, rs_side) {
        (Ok(py), Ok(rs)) => Ok((py_parsed, py, rs)),
        (Err(err), _) | (_, Err(err)) => Err(err),
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
    parsed: Vec<ParsedRustFile>,
    rs_files: &[PathBuf],
    opts: &AnalyzeOptions<'_>,
) -> Result<RsSide, kiss::RoleBuildError> {
    let t1 = std::time::Instant::now();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let roles = kiss::code_roles::classify_rust(&refs, rs_files)?;
    let t2 = std::time::Instant::now();
    let (units, stmts, viols) = analyze_rs_parsed(&parsed, opts.rs_config, &roles)?;
    let t3 = std::time::Instant::now();
    let comments = rs_comments(&parsed, opts, &roles);
    let (graph, _ctx) = build_rs_graphs(&parsed, &roles);
    let analysis = RustAnalysis {
        graph,
        dups: rs_dups(&parsed, opts, &roles),
    };
    if opts.show_timing {
        eprintln!(
            "[TIMING] rs_side roles={:.2}s analyze={:.2}s graph+={:.2}s",
            t2.duration_since(t1).as_secs_f64(),
            t3.duration_since(t2).as_secs_f64(),
            t3.elapsed().as_secs_f64()
        );
    }
    Ok(RsSide {
        parsed,
        roles,
        units,
        stmts,
        viols,
        comments,
        analysis,
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

fn rs_comments(
    parsed: &[ParsedRustFile],
    opts: &AnalyzeOptions<'_>,
    roles: &SourceRoleIndex,
) -> Vec<Violation> {
    if opts.gate_config.comment_removal_enabled {
        kiss::collect_comment_violations_with_roles(&[], parsed, Some(roles))
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

fn rs_dups(
    parsed: &[ParsedRustFile],
    opts: &AnalyzeOptions<'_>,
    roles: &SourceRoleIndex,
) -> Vec<DuplicateCluster> {
    if opts.gate_config.duplication_enabled {
        detect_rs_duplicates(parsed, opts.gate_config.min_similarity, roles)
    } else {
        Vec::new()
    }
}
