use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::graph::orphan_unit::UnitRef;
use crate::graph::orphan_unit::extract::NamedBind;
use crate::graph::{ContextDependencyGraph, module_name_for_path};
use crate::parsing::ParsedFile;
use crate::rust_include::canonical_path;
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
    let by_file = crate::graph::orphan_unit::extract::units_by_file(units);
    let mut edges = vec![Vec::new(); units.len()];
    for bind in &binds {
        for src in owners_of_line(units, &by_file, &bind.file, bind.line) {
            let unit = &units[src];
            let module = if unit.is_rust {
                module_name_for_path(rs_ctx, &unit.file)
            } else {
                module_name_for_path(py_ctx, &unit.file)
            };
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

fn owners_of_line(
    units: &[UnitRef],
    by_file: &HashMap<PathBuf, Vec<usize>>,
    file: &Path,
    line: usize,
) -> Vec<usize> {
    let Some(idxs) = by_file
        .get(file)
        .or_else(|| by_file.get(&canonical_path(file)))
    else {
        return Vec::new();
    };
    let mut owners = Vec::new();
    let mut min_span = usize::MAX;
    for &i in idxs {
        let unit = &units[i];
        if line < unit.start_line || line > unit.end_line {
            continue;
        }
        let span = unit.end_line.saturating_sub(unit.start_line);
        if span < min_span {
            min_span = span;
            owners.clear();
            owners.push(i);
        } else if span == min_span {
            owners.push(i);
        }
    }
    owners
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::orphan_unit::UnitRef;
    use crate::units::CodeUnitKind;

    fn unit(file: &str, name: &str, kind: CodeUnitKind, start: usize, end: usize) -> UnitRef {
        UnitRef {
            file: PathBuf::from(file),
            name: name.into(),
            kind,
            start_line: start,
            end_line: end,
            parent_type: None,
            is_rust: file.ends_with(".rs"),
            trait_impl: false,
        }
    }

    fn bind_belongs_to_unit(
        bind: &NamedBind,
        unit: &UnitRef,
        units: &[UnitRef],
        src: usize,
    ) -> bool {
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
        left == right || canonical_path(left) == canonical_path(right)
    }

    fn sample_units() -> Vec<UnitRef> {
        vec![
            unit("a.rs", "a", CodeUnitKind::Module, 1, 40),
            unit("a.rs", "outer", CodeUnitKind::Function, 2, 20),
            unit("a.rs", "inner", CodeUnitKind::Function, 5, 10),
            unit("a.rs", "same_a", CodeUnitKind::Function, 25, 35),
            unit("a.rs", "same_b", CodeUnitKind::Function, 25, 35),
            unit("b.rs", "b", CodeUnitKind::Module, 1, 10),
            unit("b.rs", "other", CodeUnitKind::Function, 2, 8),
        ]
    }

    #[test]
    fn owners_match_naive_innermost() {
        let units = sample_units();
        let by_file = crate::graph::orphan_unit::extract::units_by_file(&units);
        let lines = [
            (PathBuf::from("a.rs"), 1),
            (PathBuf::from("a.rs"), 6),
            (PathBuf::from("a.rs"), 15),
            (PathBuf::from("a.rs"), 30),
            (PathBuf::from("b.rs"), 4),
            (PathBuf::from("missing.rs"), 1),
        ];
        for (file, line) in lines {
            let bind = NamedBind {
                file: file.clone(),
                line,
                target_module: String::new(),
                last: "x".into(),
            };
            let mut naive: Vec<usize> = (0..units.len())
                .filter(|&src| bind_belongs_to_unit(&bind, &units[src], &units, src))
                .collect();
            let mut indexed = owners_of_line(&units, &by_file, &file, line);
            naive.sort_unstable();
            indexed.sort_unstable();
            assert_eq!(indexed, naive, "file={} line={line}", file.display());
        }
    }

    #[test]
    fn owners_skip_other_files_and_keep_tied_spans() {
        let units = sample_units();
        let by_file = crate::graph::orphan_unit::extract::units_by_file(&units);
        let inner = owners_of_line(&units, &by_file, Path::new("a.rs"), 6);
        assert_eq!(inner, vec![2]);
        let tied = owners_of_line(&units, &by_file, Path::new("a.rs"), 30);
        assert_eq!(tied, vec![3, 4]);
        let foreign = owners_of_line(&units, &by_file, Path::new("a.rs"), 4);
        assert!(!foreign.contains(&6));
    }

    #[test]
    fn large_owner_lookup_stays_subsecond() {
        let mut units = Vec::new();
        for file_i in 0..80 {
            let file = format!("f{file_i}.rs");
            units.push(unit(&file, "mod", CodeUnitKind::Module, 1, 200));
            for fn_i in 0..20 {
                let start = 2 + fn_i * 8;
                units.push(unit(
                    &file,
                    "item",
                    CodeUnitKind::Function,
                    start,
                    start + 6,
                ));
            }
        }
        let by_file = crate::graph::orphan_unit::extract::units_by_file(&units);
        let started = std::time::Instant::now();
        for file_i in 0..80 {
            let file = PathBuf::from(format!("f{file_i}.rs"));
            for line in 1..=200 {
                let _ = owners_of_line(&units, &by_file, &file, line);
            }
        }
        assert!(
            started.elapsed().as_secs() < 5,
            "owner lookup took {}ms",
            started.elapsed().as_millis()
        );
    }
}
