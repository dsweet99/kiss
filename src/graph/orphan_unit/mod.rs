use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::code_roles::SourceRoleIndex;
use crate::graph::ContextDependencyGraph;
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::units::CodeUnitKind;
use crate::violation::Violation;

mod candidates;
mod decide;
mod extract;
mod name_refs;
mod report;

pub struct OrphanCoverage {
    pub coverable: BTreeMap<PathBuf, BTreeSet<usize>>,
    pub hit: BTreeMap<PathBuf, BTreeSet<usize>>,
}

pub struct OrphanUnitInput<'a> {
    pub py: &'a [ParsedFile],
    pub rs: &'a [ParsedRustFile],
    pub py_ctx: &'a ContextDependencyGraph,
    pub rs_ctx: &'a ContextDependencyGraph,
    pub entries: &'a HashSet<PathBuf>,
    pub orphan_allowed: &'a [String],
    pub repo_root: &'a Path,
    pub roles: &'a SourceRoleIndex,
    pub coverage: Option<&'a OrphanCoverage>,
}

#[derive(Clone, Debug)]
pub(crate) struct UnitRef {
    pub file: PathBuf,
    pub name: String,
    pub kind: CodeUnitKind,
    pub start_line: usize,
    pub end_line: usize,
    pub parent_type: Option<String>,
    pub is_rust: bool,
    pub trait_impl: bool,
}

#[must_use]
pub fn orphan_unit_violations(input: &OrphanUnitInput<'_>) -> Vec<Violation> {
    let Some(coverage) = input.coverage else {
        return Vec::new();
    };
    let units = extract::collect_units(input.py, input.rs);
    let binds = extract::collect_binds(input.py, input.rs, input.py_ctx, input.rs_ctx);
    let coverage_off = extract::rust_coverage_off(input.rs);
    let graph_ok = decide::graph_witnesses(&units, input.py_ctx, input.rs_ctx, &binds);
    let cov_ok = decide::coverage_witnesses(&units, coverage);
    let cand_idx: Vec<usize> = units
        .iter()
        .enumerate()
        .filter(|(_, unit)| {
            candidates::is_candidate(candidates::CandidateIn {
                unit,
                roles: input.roles,
                entries: input.entries,
                orphan_allowed: input.orphan_allowed,
                repo_root: input.repo_root,
                coverage,
                coverage_off: &coverage_off,
            })
        })
        .map(|(i, _)| i)
        .collect();
    let candidates: Vec<UnitRef> = cand_idx.iter().map(|&i| units[i].clone()).collect();
    let orphans: Vec<&UnitRef> = cand_idx
        .iter()
        .filter(|&&i| !graph_ok[i] && !cov_ok[i])
        .map(|&i| &units[i])
        .collect();
    report::to_violations(&candidates, &orphans)
}

#[cfg(test)]
#[path = "orphan_unit_test.rs"]
mod orphan_unit_test;
