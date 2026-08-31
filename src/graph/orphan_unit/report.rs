use std::collections::HashMap;
use std::path::PathBuf;

use crate::graph::orphan_unit::UnitRef;
use crate::violation::Violation;

pub(super) fn to_violations(candidates: &[UnitRef], orphans: &[&UnitRef]) -> Vec<Violation> {
    let mut by_file: HashMap<PathBuf, usize> = HashMap::new();
    for unit in candidates {
        *by_file.entry(unit.file.clone()).or_default() += 1;
    }
    let mut orphan_by_file: HashMap<PathBuf, usize> = HashMap::new();
    for unit in orphans {
        *orphan_by_file.entry(unit.file.clone()).or_default() += 1;
    }
    let mut out = Vec::new();
    let mut reported_file = HashMap::new();
    for unit in orphans {
        let cand = by_file.get(&unit.file).copied().unwrap_or(0);
        let orph = orphan_by_file.get(&unit.file).copied().unwrap_or(0);
        if cand > 0 && cand == orph {
            if reported_file.insert(unit.file.clone(), true).is_some() {
                continue;
            }
            out.push(file_violation(unit));
            continue;
        }
        out.push(unit_violation(unit));
    }
    out.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.unit_name.cmp(&b.unit_name))
    });
    out
}

fn unit_violation(unit: &UnitRef) -> Violation {
    Violation {
        file: unit.file.clone(),
        line: unit.start_line,
        unit_name: unit.name.clone(),
        metric: "orphan".to_string(),
        value: 0,
        threshold: 0,
        message: format!(
            "{} '{}' is unused: no named import/use and no runtime coverable line ran.",
            unit.kind, unit.name
        ),
        suggestion: "Import or call this unit from production or test code, or delete it."
            .to_string(),
    }
}

fn file_violation(unit: &UnitRef) -> Violation {
    let name = unit
        .file
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| unit.name.clone());
    Violation {
        file: unit.file.clone(),
        line: 1,
        unit_name: name,
        metric: "orphan".to_string(),
        value: 0,
        threshold: 0,
        message: "every candidate unit in this file is unused: no named import/use and no runtime coverable line ran."
            .to_string(),
        suggestion: "Import or call this file from production or test code, or delete it."
            .to_string(),
    }
}
