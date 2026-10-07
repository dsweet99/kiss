use crate::graph::orphan_unit::UnitRef;
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
) -> Vec<bool> {
    let mut reached = vec![false; units.len()];
    let mut queue = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        if is_root(unit, input) {
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

fn is_root(unit: &UnitRef, input: &crate::graph::orphan_unit::OrphanUnitInput<'_>) -> bool {
    if unit.trait_impl {
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
        let contains = unit.kind == CodeUnitKind::Module
            || (unit.kind == CodeUnitKind::Class
                && unit.start_line <= child.start_line
                && child.end_line <= unit.end_line);
        if contains {
            reached[i] = true;
            queue.push(i);
        }
    }
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
