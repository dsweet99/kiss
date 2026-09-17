use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::graph::orphan_unit::extract::file_key;
use crate::graph::orphan_unit::{OrphanCoverage, UnitRef};
use crate::rust_include::canonical_path;
use crate::units::CodeUnitKind;

#[cfg(test)]
mod bind_index {
    use crate::graph::orphan_unit::extract::NamedBind;
    use std::collections::HashSet;

    pub(super) struct BindIndex {
        pub empty_last: HashSet<String>,
        pub named: HashSet<(String, String)>,
        pub lasts: HashSet<String>,
        pub targets: HashSet<String>,
    }

    impl BindIndex {
        pub fn new(binds: &[NamedBind]) -> Self {
            let mut empty_last = HashSet::new();
            let mut named = HashSet::new();
            let mut lasts = HashSet::new();
            let mut targets = HashSet::new();
            for bind in binds {
                lasts.insert(bind.last.clone());
                if bind.target_module.is_empty() {
                    empty_last.insert(bind.last.clone());
                } else {
                    targets.insert(bind.target_module.clone());
                    named.insert((bind.last.clone(), bind.target_module.clone()));
                }
            }
            Self {
                empty_last,
                named,
                lasts,
                targets,
            }
        }
    }
}

pub(super) fn flood_reached(
    units: &[UnitRef],
    edges: &[Vec<usize>],
    input: &crate::graph::orphan_unit::OrphanUnitInput<'_>,
    coverage: &OrphanCoverage,
) -> Vec<bool> {
    let cov = coverage_witnesses(units, coverage);
    let mut reached = vec![false; units.len()];
    let mut queue = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        if is_root(unit, input, cov[i]) {
            reached[i] = true;
            mark_containers(units, i, &mut reached, &mut queue);
            queue.push(i);
        }
    }
    while let Some(src) = queue.pop() {
        for &dest in &edges[src] {
            if !reached[dest] {
                reached[dest] = true;
                mark_containers(units, dest, &mut reached, &mut queue);
                queue.push(dest);
            }
        }
    }
    reached
}

fn is_root(
    unit: &UnitRef,
    input: &crate::graph::orphan_unit::OrphanUnitInput<'_>,
    coverage_root: bool,
) -> bool {
    if coverage_root {
        return true;
    }
    if input.roles.role_at(&unit.file, unit.start_line) == crate::code_roles::CodeRole::TestOnly
        || input.roles.file_composition(&unit.file) == crate::code_roles::FileComposition::TestOnly
    {
        return true;
    }
    let canon = crate::rust_include::canonical_path(&unit.file);
    let file_is_entry = input.entries.contains(&canon) || input.entries.contains(&unit.file);
    if unit.kind == CodeUnitKind::Module && file_is_entry {
        return true;
    }
    if unit.is_rust && unit.kind == CodeUnitKind::Function && unit.name == "main" {
        return true;
    }
    input.entry_callables.iter().any(|(path, name)| {
        name == &unit.name
            && (path == &unit.file || crate::rust_include::canonical_path(path) == canon)
    })
}

fn mark_containers(units: &[UnitRef], idx: usize, reached: &mut [bool], queue: &mut Vec<usize>) {
    let child = &units[idx];
    for (i, unit) in units.iter().enumerate() {
        if i == idx || unit.file != child.file || reached[i] {
            continue;
        }
        let contains = unit.start_line <= child.start_line && child.end_line <= unit.end_line;
        let is_container = matches!(unit.kind, CodeUnitKind::Class | CodeUnitKind::Module);
        if contains && is_container {
            reached[i] = true;
            queue.push(i);
        }
    }
}

pub(super) fn coverage_witnesses(units: &[UnitRef], coverage: &OrphanCoverage) -> Vec<bool> {
    let by_file = crate::graph::orphan_unit::extract::units_by_file(units);
    let direct = direct_callable_hits(units, &by_file, coverage);
    let mut out = direct.clone();
    for (i, unit) in units.iter().enumerate() {
        if unit.kind == CodeUnitKind::Class {
            out[i] = nested_callable_hit(units, &by_file, unit, &direct)
                || exclusive_body_hit(units, &by_file, unit, coverage);
        }
    }
    for (i, unit) in units.iter().enumerate() {
        if unit.kind == CodeUnitKind::Module {
            out[i] = module_coverage(units, &by_file, unit, coverage, &out);
        }
    }
    out
}

fn file_unit_idxs<'a>(
    by_file: &'a HashMap<PathBuf, Vec<usize>>,
    file: &Path,
) -> Option<&'a [usize]> {
    by_file
        .get(file)
        .or_else(|| by_file.get(&canonical_path(file)))
        .map(Vec::as_slice)
}

fn direct_callable_hits(
    units: &[UnitRef],
    by_file: &HashMap<PathBuf, Vec<usize>>,
    coverage: &OrphanCoverage,
) -> Vec<bool> {
    let mut out = vec![false; units.len()];
    for (file, coverable) in &coverage.coverable {
        let hits = file_key(&coverage.hit, file).cloned().unwrap_or_default();
        for line in coverable {
            if !hits.contains(line) {
                continue;
            }
            let Some(idx) = innermost(units, by_file, file, *line) else {
                continue;
            };
            let unit = &units[idx];
            if !is_callable(unit.kind) {
                continue;
            }
            if !unit.is_rust && *line == unit.start_line {
                continue;
            }
            out[idx] = true;
        }
    }
    out
}

fn is_callable(kind: CodeUnitKind) -> bool {
    matches!(
        kind,
        CodeUnitKind::Function | CodeUnitKind::Method | CodeUnitKind::TraitImplMethod
    )
}

fn nested_callable_hit(
    units: &[UnitRef],
    by_file: &HashMap<PathBuf, Vec<usize>>,
    class: &UnitRef,
    direct: &[bool],
) -> bool {
    let Some(idxs) = file_unit_idxs(by_file, &class.file) else {
        return false;
    };
    idxs.iter().any(|&i| {
        direct[i]
            && units[i].parent_type.as_deref() == Some(class.name.as_str())
            && is_callable(units[i].kind)
    })
}

fn module_coverage(
    units: &[UnitRef],
    by_file: &HashMap<PathBuf, Vec<usize>>,
    module: &UnitRef,
    coverage: &OrphanCoverage,
    witnessed: &[bool],
) -> bool {
    if let Some(idxs) = file_unit_idxs(by_file, &module.file)
        && idxs
            .iter()
            .any(|&i| witnessed[i] && units[i].kind != CodeUnitKind::Module)
    {
        return true;
    }
    exclusive_body_hit(units, by_file, module, coverage)
}

fn exclusive_body_hit(
    units: &[UnitRef],
    by_file: &HashMap<PathBuf, Vec<usize>>,
    unit: &UnitRef,
    coverage: &OrphanCoverage,
) -> bool {
    let Some(coverable) = file_key(&coverage.coverable, &unit.file) else {
        return false;
    };
    let hits = file_key(&coverage.hit, &unit.file)
        .cloned()
        .unwrap_or_default();
    coverable.iter().any(|line| {
        if !hits.contains(line) {
            return false;
        }
        let Some(idx) = innermost(units, by_file, &unit.file, *line) else {
            return false;
        };
        units[idx].start_line == unit.start_line
            && units[idx].end_line == unit.end_line
            && units[idx].kind == unit.kind
    })
}

fn innermost(
    units: &[UnitRef],
    by_file: &HashMap<PathBuf, Vec<usize>>,
    file: &Path,
    line: usize,
) -> Option<usize> {
    let idxs = file_unit_idxs(by_file, file)?;
    idxs.iter()
        .copied()
        .filter(|&i| units[i].start_line <= line && line <= units[i].end_line)
        .min_by_key(|&i| {
            (
                units[i].end_line.saturating_sub(units[i].start_line),
                usize::MAX - units[i].start_line,
            )
        })
}

#[cfg(test)]
mod bind_index_test {
    use super::bind_index::BindIndex;
    use crate::graph::orphan_unit::extract::NamedBind;

    fn linear_nested(name: &str, module: &str, binds: &[NamedBind]) -> bool {
        binds.iter().any(|bind| {
            bind.last == name && (bind.target_module.is_empty() || bind.target_module == module)
        })
    }

    fn linear_module(module: &str, binds: &[NamedBind]) -> bool {
        binds
            .iter()
            .any(|bind| bind.target_module == module || bind.last == module)
    }

    #[test]
    fn bind_index_matches_linear_scan() {
        let file = std::path::PathBuf::from("a.py");
        let binds = [
            NamedBind {
                file: file.clone(),
                line: 1,
                target_module: String::new(),
                last: "foo".into(),
            },
            NamedBind {
                file: file.clone(),
                line: 2,
                target_module: "m".into(),
                last: "bar".into(),
            },
            NamedBind {
                file: file.clone(),
                line: 3,
                target_module: "other".into(),
                last: "bar".into(),
            },
            NamedBind {
                file,
                line: 4,
                target_module: "m".into(),
                last: "m".into(),
            },
        ];
        let idx = BindIndex::new(&binds);
        for (name, module) in [
            ("foo", "m"),
            ("bar", "m"),
            ("bar", "other"),
            ("baz", "m"),
            ("foo", "other"),
        ] {
            let indexed = idx.empty_last.contains(name)
                || idx.named.contains(&(name.to_string(), module.to_string()));
            assert_eq!(
                indexed,
                linear_nested(name, module, &binds),
                "{name} in {module}"
            );
        }
        for module in ["m", "other", "missing"] {
            let indexed = idx.targets.contains(module) || idx.lasts.contains(module);
            assert_eq!(indexed, linear_module(module, &binds), "module {module}");
        }
    }
}
