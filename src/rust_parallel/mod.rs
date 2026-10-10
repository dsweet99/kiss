mod facts;
mod pool;

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::DuplicateCluster;
use crate::code_roles::{RoleBuildError, SourceRoleIndex, classify_rust_with_known};
use crate::config::Config;
use crate::gate_config::GateConfig;
use crate::graph::DependencyGraph;
use crate::violation::Violation;

use facts::assemble_output;
use pool::Pool;

pub struct ParallelRustRequest<'a> {
    pub files: &'a [PathBuf],
    pub config: &'a Config,
    pub gate: &'a GateConfig,
    pub repo_root: &'a Path,
    pub show_timing: bool,
}

pub struct ParallelRustOutput {
    pub sources: Vec<(PathBuf, String)>,
    pub roles: SourceRoleIndex,
    pub units: usize,
    pub stmts: usize,
    pub viols: Vec<Violation>,
    pub comments: Vec<Violation>,
    pub docs: Vec<Violation>,
    pub graph: Option<DependencyGraph>,
    pub dups: Vec<DuplicateCluster>,
}

pub fn parallel_rust_analysis(
    req: ParallelRustRequest<'_>,
) -> Result<ParallelRustOutput, RoleBuildError> {
    if req.files.is_empty() {
        return Ok(empty_output());
    }
    let pool = Pool::open(req.files)?;
    let t_roles = Instant::now();
    let roles = classify_on_pool(&pool, req.files)?;
    let roles_s = t_roles.elapsed().as_secs_f64();
    let t_analyze = Instant::now();
    let facts = facts::map_facts(&pool, &roles, req.config, req.gate, req.repo_root)?;
    let analyze_s = t_analyze.elapsed().as_secs_f64();
    let sources = pool.map(|parsed| parsed.source.clone())?;
    let t_rest = Instant::now();
    let output = assemble_output(
        req.files,
        roles,
        facts,
        sources,
        req.config,
        req.gate.min_similarity,
    );
    if req.show_timing {
        eprintln!(
            "[TIMING] rs_side roles={roles_s:.2}s analyze={analyze_s:.2}s graph+={:.2}s",
            t_rest.elapsed().as_secs_f64()
        );
    }
    Ok(output)
}

fn empty_output() -> ParallelRustOutput {
    ParallelRustOutput {
        sources: Vec::new(),
        roles: SourceRoleIndex::empty(),
        units: 0,
        stmts: 0,
        viols: Vec::new(),
        comments: Vec::new(),
        docs: Vec::new(),
        graph: None,
        dups: Vec::new(),
    }
}

fn classify_on_pool(pool: &Pool, files: &[PathBuf]) -> Result<SourceRoleIndex, RoleBuildError> {
    classify_rust_with_known(&[], files, &mut |path, pred, allow, atoms| {
        pool.walk(path, pred, allow, atoms)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_roles::classify_rust;
    use crate::rust_parsing::parse_rust_files;

    #[test]
    fn parallel_roles_match_sequential_cfg_mod() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib.rs");
        let helper = tmp.path().join("helper.rs");
        std::fs::write(&lib, "pub fn prod() {}\n#[cfg(test)]\nmod helper;\n").unwrap();
        std::fs::write(&helper, "pub fn support() {}\n").unwrap();
        let files = vec![lib.clone(), helper.clone()];
        let sequential = {
            let parsed = parse_rust_files(&files);
            let parsed: Vec<_> = parsed.into_iter().map(|r| r.unwrap()).collect();
            let refs: Vec<_> = parsed.iter().collect();
            classify_rust(&refs, &files).unwrap()
        };
        let parallel = classify_on_pool(&Pool::open(&files).unwrap(), &files).unwrap();
        assert_eq!(
            sequential.file_composition(&helper),
            parallel.file_composition(&helper)
        );
        assert_eq!(sequential.role_at(&lib, 1), parallel.role_at(&lib, 1));
    }
}
