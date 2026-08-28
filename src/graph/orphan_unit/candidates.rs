use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::code_roles::{CodeRole, SourceRoleIndex};
use crate::comments::{normalize_allowed_dirs, path_in_allowed_dirs};
use crate::graph::orphan_unit::extract::file_key;
use crate::graph::orphan_unit::{OrphanCoverage, UnitRef};
use crate::rust_include::canonical_path;
use crate::units::CodeUnitKind;

pub(super) struct CandidateIn<'a> {
    pub unit: &'a UnitRef,
    pub roles: &'a SourceRoleIndex,
    pub entries: &'a HashSet<PathBuf>,
    pub orphan_allowed: &'a [String],
    pub repo_root: &'a Path,
    pub coverage: &'a OrphanCoverage,
    pub coverage_off: &'a HashSet<(PathBuf, String, usize)>,
}

pub(super) fn is_candidate(in_: CandidateIn<'_>) -> bool {
    let path = &in_.unit.file;
    if role_or_path_excluded(&in_, path) || unit_kind_excluded(in_.unit, in_.entries) {
        return false;
    }
    let prod_coverable = production_coverable(in_.unit, in_.roles, in_.coverage);
    let off =
        in_.coverage_off
            .contains(&(path.clone(), in_.unit.name.clone(), in_.unit.start_line));
    !(prod_coverable.is_empty() && off)
}

fn role_or_path_excluded(in_: &CandidateIn<'_>, path: &Path) -> bool {
    if in_.roles.file_composition(path) == crate::code_roles::FileComposition::TestOnly {
        return true;
    }
    if in_.roles.role_at(path, in_.unit.start_line) == CodeRole::TestOnly {
        return true;
    }
    let allowed = normalize_allowed_dirs(in_.orphan_allowed);
    path_in_allowed_dirs(path, in_.repo_root, &allowed)
}

fn unit_kind_excluded(unit: &UnitRef, entries: &HashSet<PathBuf>) -> bool {
    unit.trait_impl || is_init_module_unit(unit) || is_entry_unit(unit, entries)
}

fn is_init_module_unit(unit: &UnitRef) -> bool {
    unit.kind == CodeUnitKind::Module
        && unit
            .file
            .file_stem()
            .is_some_and(|stem| stem == "__init__" || stem == "mod")
}

fn is_entry_unit(unit: &UnitRef, entries: &HashSet<PathBuf>) -> bool {
    let canon = canonical_path(&unit.file);
    let file_is_entry = entries.contains(&canon) || entries.contains(&unit.file);
    if unit.kind == CodeUnitKind::Module && file_is_entry {
        return true;
    }
    unit.is_rust && unit.kind == CodeUnitKind::Function && unit.name == "main"
}

fn production_coverable(
    unit: &UnitRef,
    roles: &SourceRoleIndex,
    coverage: &OrphanCoverage,
) -> Vec<usize> {
    let Some(lines) = file_key(&coverage.coverable, &unit.file) else {
        return Vec::new();
    };
    let in_range: Vec<usize> = lines
        .iter()
        .copied()
        .filter(|line| *line >= unit.start_line && *line <= unit.end_line)
        .collect();
    roles.production_lines(&unit.file, &in_range)
}
