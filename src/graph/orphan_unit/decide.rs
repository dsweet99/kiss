use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::graph::orphan_unit::extract::{NamedBind, file_key};
use crate::graph::orphan_unit::{OrphanCoverage, UnitRef};
use crate::graph::{ContextDependencyGraph, DependencyGraph, module_name_for_path};
use crate::rust_include::canonical_path;
use crate::units::CodeUnitKind;

struct BindIndex {
    empty_last: HashSet<String>,
    named: HashSet<(String, String)>,
    lasts: HashSet<String>,
    targets: HashSet<String>,
}

impl BindIndex {
    fn new(binds: &[NamedBind]) -> Self {
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

pub(super) fn graph_witnesses(
    units: &[UnitRef],
    py_ctx: &ContextDependencyGraph,
    rs_ctx: &ContextDependencyGraph,
    binds: &[NamedBind],
) -> Vec<bool> {
    let index = BindIndex::new(binds);
    let py_prod = py_ctx.production_view();
    let rs_prod = rs_ctx.production_view();
    units
        .iter()
        .map(|unit| {
            if unit.is_rust {
                graph_witness(unit, rs_ctx, &rs_prod, &index)
            } else {
                graph_witness(unit, py_ctx, &py_prod, &index)
            }
        })
        .collect()
}

fn graph_witness(
    unit: &UnitRef,
    ctx: &ContextDependencyGraph,
    prod: &DependencyGraph,
    binds: &BindIndex,
) -> bool {
    let Some(module) = module_name_for_path(ctx, &unit.file) else {
        return false;
    };
    if unit.kind == CodeUnitKind::Module {
        return module_graph_witness(ctx, prod, &module)
            || binds.targets.contains(&module)
            || binds.lasts.contains(&module);
    }
    binds.empty_last.contains(&unit.name) || binds.named.contains(&(unit.name.clone(), module))
}

fn module_graph_witness(
    ctx: &ContextDependencyGraph,
    prod: &DependencyGraph,
    module: &str,
) -> bool {
    let fan_in = prod.module_metrics(module).fan_in;
    let has_test_importer = !ctx.test_importers_of(module).is_empty();
    !crate::graph::GraphIsolation::UnreferencedModule.module_is_isolated(
        fan_in,
        0,
        has_test_importer,
    )
}

pub(super) fn coverage_witnesses(units: &[UnitRef], coverage: &OrphanCoverage) -> Vec<bool> {
    let by_file = units_by_file(units);
    let direct = direct_callable_hits(units, &by_file, coverage);
    let mut out = direct.clone();
    for (i, unit) in units.iter().enumerate() {
        if unit.kind == CodeUnitKind::Class {
            out[i] = nested_callable_hit(units, &by_file, unit, &direct);
        }
    }
    for (i, unit) in units.iter().enumerate() {
        if unit.kind == CodeUnitKind::Module {
            out[i] = module_coverage(units, &by_file, unit, coverage, &out);
        }
    }
    out
}

fn units_by_file(units: &[UnitRef]) -> HashMap<PathBuf, Vec<usize>> {
    let mut map: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for (i, unit) in units.iter().enumerate() {
        map.entry(unit.file.clone()).or_default().push(i);
    }
    let aliases: Vec<(PathBuf, Vec<usize>)> = map
        .iter()
        .filter_map(|(path, idxs)| {
            let canon = canonical_path(path);
            (!map.contains_key(&canon)).then(|| (canon, idxs.clone()))
        })
        .collect();
    map.extend(aliases);
    map
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
    let Some(coverable) = file_key(&coverage.coverable, &module.file) else {
        return false;
    };
    let hits = file_key(&coverage.hit, &module.file)
        .cloned()
        .unwrap_or_default();
    coverable.iter().any(|line| {
        if !hits.contains(line) {
            return false;
        }
        let Some(idx) = innermost(units, by_file, &module.file, *line) else {
            return false;
        };
        units[idx].kind == CodeUnitKind::Module
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
    use super::{BindIndex, NamedBind};

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
        let binds = [
            NamedBind {
                target_module: String::new(),
                last: "foo".into(),
            },
            NamedBind {
                target_module: "m".into(),
                last: "bar".into(),
            },
            NamedBind {
                target_module: "other".into(),
                last: "bar".into(),
            },
            NamedBind {
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
