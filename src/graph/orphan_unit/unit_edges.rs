use std::collections::HashMap;
use std::path::Path;

use crate::graph::orphan_unit::UnitRef;
use crate::graph::orphan_unit::extract::NamedBind;
use crate::graph::{ContextDependencyGraph, module_name_for_path};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::units::CodeUnitKind;

pub(super) fn edges_from_units(
    units: &[UnitRef],
    py: &[ParsedFile],
    rs: &[ParsedRustFile],
    py_ctx: &ContextDependencyGraph,
    rs_ctx: &ContextDependencyGraph,
) -> Vec<Vec<usize>> {
    let binds = crate::graph::orphan_unit::extract::collect_binds(py, rs, py_ctx, rs_ctx);
    let by_name = name_index(units);
    let mut edges = vec![Vec::new(); units.len()];
    for (src, unit) in units.iter().enumerate() {
        let module = if unit.is_rust {
            module_name_for_path(rs_ctx, &unit.file)
        } else {
            module_name_for_path(py_ctx, &unit.file)
        };
        for bind in binds_for_unit(&binds, units, src) {
            for dest in resolve_targets(&by_name, bind, module.as_deref()) {
                if dest != src {
                    edges[src].push(dest);
                }
            }
        }
    }
    for dests in &mut edges {
        dests.sort_unstable();
        dests.dedup();
    }
    edges
}

fn binds_for_unit<'a>(binds: &'a [NamedBind], units: &[UnitRef], src: usize) -> Vec<&'a NamedBind> {
    let unit = &units[src];
    binds
        .iter()
        .filter(|bind| bind_belongs_to_unit(bind, unit, units, src))
        .collect()
}

fn bind_belongs_to_unit(bind: &NamedBind, unit: &UnitRef, units: &[UnitRef], src: usize) -> bool {
    if !same_file(&bind.file, &unit.file) {
        return false;
    }
    if bind.line < unit.start_line || bind.line > unit.end_line {
        return false;
    }
    !units.iter().enumerate().any(|(idx, other)| {
        idx != src
            && same_file(&other.file, &unit.file)
            && other.start_line >= unit.start_line
            && other.end_line <= unit.end_line
            && other.start_line <= bind.line
            && bind.line <= other.end_line
            && (other.end_line.saturating_sub(other.start_line)
                < unit.end_line.saturating_sub(unit.start_line))
    })
}

fn same_file(left: &Path, right: &Path) -> bool {
    left == right
        || crate::rust_include::canonical_path(left) == crate::rust_include::canonical_path(right)
}

fn name_index(units: &[UnitRef]) -> HashMap<String, Vec<usize>> {
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, unit) in units.iter().enumerate() {
        map.entry(unit.name.clone()).or_default().push(i);
        if unit.kind == CodeUnitKind::Module
            && let Some(stem) = unit.file.file_stem()
        {
            map.entry(stem.to_string_lossy().into_owned())
                .or_default()
                .push(i);
        }
    }
    map
}

fn resolve_targets(
    by_name: &HashMap<String, Vec<usize>>,
    bind: &NamedBind,
    src_module: Option<&str>,
) -> Vec<usize> {
    let Some(hits) = by_name.get(&bind.last) else {
        return Vec::new();
    };
    if bind.target_module.is_empty() {
        return hits.clone();
    }
    let narrowed: Vec<usize> = hits
        .iter()
        .copied()
        .filter(|&_i| {
            let _ = src_module;
            true
        })
        .collect();
    if narrowed.is_empty() {
        hits.clone()
    } else {
        narrowed
    }
}
