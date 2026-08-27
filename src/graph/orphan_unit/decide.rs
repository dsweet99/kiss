use std::path::Path;

use crate::graph::orphan_unit::extract::{file_key, NamedBind};
use crate::graph::orphan_unit::{OrphanCoverage, UnitRef};
use crate::graph::{module_name_for_path, ContextDependencyGraph};
use crate::rust_include::canonical_path;
use crate::units::CodeUnitKind;

pub(super) fn graph_witnesses(
    units: &[UnitRef],
    py_ctx: &ContextDependencyGraph,
    rs_ctx: &ContextDependencyGraph,
    binds: &[NamedBind],
) -> Vec<bool> {
    units
        .iter()
        .map(|unit| {
            let ctx = if unit.is_rust { rs_ctx } else { py_ctx };
            graph_witness(unit, ctx, binds)
        })
        .collect()
}

fn graph_witness(unit: &UnitRef, ctx: &ContextDependencyGraph, binds: &[NamedBind]) -> bool {
    let Some(module) = module_name_for_path(ctx, &unit.file) else {
        return false;
    };
    if unit.kind == CodeUnitKind::Module {
        return module_graph_witness(ctx, &module);
    }
    binds
        .iter()
        .any(|bind| bind.target_module == module && bind.last == unit.name)
}

fn module_graph_witness(ctx: &ContextDependencyGraph, module: &str) -> bool {
    let fan_in = ctx.production_view().module_metrics(module).fan_in;
    let has_test_importer = !ctx.test_importers_of(module).is_empty();
    !crate::graph::GraphIsolation::UnreferencedModule.module_is_isolated(
        fan_in,
        0,
        has_test_importer,
    )
}

pub(super) fn coverage_witnesses(units: &[UnitRef], coverage: &OrphanCoverage) -> Vec<bool> {
    let direct = direct_callable_hits(units, coverage);
    let mut out = direct.clone();
    for (i, unit) in units.iter().enumerate() {
        if unit.kind == CodeUnitKind::Class {
            out[i] = nested_callable_hit(units, unit, &direct);
        }
    }
    for (i, unit) in units.iter().enumerate() {
        if unit.kind == CodeUnitKind::Module {
            out[i] = module_coverage(units, unit, coverage, &out);
        }
    }
    out
}

fn direct_callable_hits(units: &[UnitRef], coverage: &OrphanCoverage) -> Vec<bool> {
    let mut out = vec![false; units.len()];
    for (file, coverable) in &coverage.coverable {
        let hits = file_key(&coverage.hit, file).cloned().unwrap_or_default();
        for line in coverable {
            if !hits.contains(line) {
                continue;
            }
            let Some(idx) = innermost(units, file, *line) else {
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

fn nested_callable_hit(units: &[UnitRef], class: &UnitRef, direct: &[bool]) -> bool {
    units.iter().enumerate().any(|(i, unit)| {
        direct[i]
            && paths_eq(&unit.file, &class.file)
            && unit.parent_type.as_deref() == Some(class.name.as_str())
            && is_callable(unit.kind)
    })
}

fn module_coverage(
    units: &[UnitRef],
    module: &UnitRef,
    coverage: &OrphanCoverage,
    witnessed: &[bool],
) -> bool {
    if units.iter().enumerate().any(|(i, unit)| {
        witnessed[i] && paths_eq(&unit.file, &module.file) && unit.kind != CodeUnitKind::Module
    }) {
        return true;
    }
    let Some(coverable) = file_key(&coverage.coverable, &module.file) else {
        return false;
    };
    let hits = file_key(&coverage.hit, &module.file).cloned().unwrap_or_default();
    coverable.iter().any(|line| {
        if !hits.contains(line) {
            return false;
        }
        let Some(idx) = innermost(units, &module.file, *line) else {
            return false;
        };
        units[idx].kind == CodeUnitKind::Module
    })
}

fn innermost(units: &[UnitRef], file: &Path, line: usize) -> Option<usize> {
    units
        .iter()
        .enumerate()
        .filter(|(_, unit)| paths_eq(&unit.file, file) && unit.start_line <= line && line <= unit.end_line)
        .min_by_key(|(_, unit)| (unit.end_line.saturating_sub(unit.start_line), usize::MAX - unit.start_line))
        .map(|(i, _)| i)
}

fn paths_eq(a: &Path, b: &Path) -> bool {
    a == b || canonical_path(a) == canonical_path(b)
}
