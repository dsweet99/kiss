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
) -> Result<(ParallelRustOutput, Vec<RoleBuildError>), RoleBuildError> {
    if req.files.is_empty() {
        return Ok((empty_output(), Vec::new()));
    }
    let (pool, errors) = Pool::open(req.files)?;
    let good: Vec<PathBuf> = req
        .files
        .iter()
        .filter(|path| pool.contains(path))
        .cloned()
        .collect();
    if good.is_empty() {
        return Ok((empty_output(), errors));
    }
    let t_roles = Instant::now();
    let roles = match classify_on_pool(&pool, &good) {
        Ok(roles) => roles,
        Err(err) => {
            for error in &errors {
                eprintln!("{error}");
            }
            return Err(err);
        }
    };
    let roles_s = t_roles.elapsed().as_secs_f64();
    let t_analyze = Instant::now();
    let facts = facts::map_facts(&pool, &roles, req.config, req.gate, req.repo_root)?;
    let analyze_s = t_analyze.elapsed().as_secs_f64();
    let sources = pool.map(|parsed| parsed.source.clone())?;
    let t_rest = Instant::now();
    let output = assemble_output(
        &good,
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
    Ok((output, errors))
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
        let (pool, errors) = Pool::open(&files).unwrap();
        assert!(errors.is_empty());
        let parallel = classify_on_pool(&pool, &files).unwrap();
        assert_eq!(
            sequential.file_composition(&helper),
            parallel.file_composition(&helper)
        );
        assert_eq!(sequential.role_at(&lib, 1), parallel.role_at(&lib, 1));
    }

    #[test]
    fn syntax_error_skips_that_file_and_keeps_its_sibling() {
        let tmp = tempfile::tempdir().unwrap();
        let good = tmp.path().join("good.rs");
        let bad = tmp.path().join("bad.rs");
        std::fs::write(&good, "pub fn ok() {}\n").unwrap();
        std::fs::write(&bad, "pub fn broken(\n").unwrap();
        let files = vec![bad, good.clone()];
        let config = crate::config::Config::rust_defaults();
        let gate = crate::gate_config::GateConfig::default();
        let (out, errors) = parallel_rust_analysis(ParallelRustRequest {
            files: &files,
            config: &config,
            gate: &gate,
            repo_root: tmp.path(),
            show_timing: false,
        })
        .unwrap();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("bad.rs"), "{errors:?}");
        assert_eq!(out.sources.len(), 1);
        assert_eq!(out.sources[0].0, good);
    }
}
