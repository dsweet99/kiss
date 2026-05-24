use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::symbol_mv::MvPlan;
use crate::symbol_mv::PlannedEdit;

pub fn apply_plan_transactional(plan: &MvPlan) -> Result<(), String> {
    check_for_overlaps(plan)?;
    let originals = read_original_snapshots(&plan.files)?;
    let mut per_file_edits = group_edits_by_path(plan);
    apply_all_file_edits(&originals, &mut per_file_edits)
}

#[derive(Clone)]
struct Snapshot {
    existed: bool,
    content: String,
}

fn read_original_snapshots(files: &[PathBuf]) -> Result<BTreeMap<PathBuf, Snapshot>, String> {
    let mut originals = BTreeMap::new();
    for path in files {
        let snapshot = match fs::read_to_string(path) {
            Ok(content) => Snapshot {
                existed: true,
                content,
            },
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Snapshot {
                        existed: false,
                        content: String::new(),
                    }
                } else {
                    return Err(format!(
                        "failed reading snapshot for {}: {e}",
                        path.display()
                    ));
                }
            }
        };
        originals.insert(path.clone(), snapshot);
    }
    Ok(originals)
}

fn group_edits_by_path(plan: &MvPlan) -> BTreeMap<PathBuf, Vec<&PlannedEdit>> {
    let mut per_file_edits: BTreeMap<PathBuf, Vec<&PlannedEdit>> = BTreeMap::new();
    for edit in &plan.edits {
        per_file_edits
            .entry(edit.path.clone())
            .or_default()
            .push(edit);
    }
    per_file_edits
}

fn apply_all_file_edits(
    originals: &BTreeMap<PathBuf, Snapshot>,
    per_file_edits: &mut BTreeMap<PathBuf, Vec<&PlannedEdit>>,
) -> Result<(), String> {
    for (path, edits) in per_file_edits.iter_mut() {
        let Some(source) = originals.get(path) else {
            return Err(format!("missing source snapshot for {}", path.display()));
        };
        if edits.is_empty() {
            continue;
        }
        apply_edits_to_one_file(originals, path, source, edits)?;
    }
    Ok(())
}

fn apply_edits_to_one_file(
    originals: &BTreeMap<PathBuf, Snapshot>,
    path: &PathBuf,
    source: &Snapshot,
    edits: &mut Vec<&PlannedEdit>,
) -> Result<(), String> {
    let mut updated = source.content.clone();
    edits.sort_by(|a, b| b.start_byte.cmp(&a.start_byte));
    for edit in edits.iter() {
        if edit.end_byte > updated.len() || edit.start_byte > edit.end_byte {
            rollback(originals)?;
            return Err(format!(
                "invalid edit range {}..{} for {}",
                edit.start_byte,
                edit.end_byte,
                path.display()
            ));
        }
        updated.replace_range(edit.start_byte..edit.end_byte, &edit.new_snippet);
    }
    if let Err(err) = fs::write(path, updated) {
        rollback(originals)?;
        return Err(format!("failed writing {}: {err}", path.display()));
    }
    Ok(())
}

fn check_for_overlaps(plan: &MvPlan) -> Result<(), String> {
    let mut by_file: BTreeMap<&PathBuf, Vec<(usize, usize)>> = BTreeMap::new();
    for edit in &plan.edits {
        by_file
            .entry(&edit.path)
            .or_default()
            .push((edit.start_byte, edit.end_byte));
    }
    for (path, mut ranges) in by_file {
        ranges.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for pair in ranges.windows(2) {
            if pair[0].1 > pair[1].0 {
                return Err(format!(
                    "overlapping edits in {}: {}..{} overlaps {}..{}",
                    path.display(),
                    pair[0].0,
                    pair[0].1,
                    pair[1].0,
                    pair[1].1
                ));
            }
        }
    }
    Ok(())
}

fn rollback(originals: &BTreeMap<PathBuf, Snapshot>) -> Result<(), String> {
    for (path, content) in originals {
        if content.existed {
            fs::write(path, &content.content)
                .map_err(|err| format!("rollback failed for {}: {err}", path.display()))?;
        } else if path.exists() {
            fs::remove_file(path)
                .map_err(|err| format!("rollback failed for {}: {err}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod transaction_coverage {
    use super::*;
    use crate::symbol_mv::{EditKind, MvPlan, PlannedEdit};
    use std::path::PathBuf;

    #[test]
    fn check_for_overlaps_rejects_overlapping_edits() {
        let p = PathBuf::from("overlap.txt");
        let plan = MvPlan {
            files: vec![p.clone()],
            edits: vec![
                PlannedEdit {
                    path: p.clone(),
                    start_byte: 0,
                    end_byte: 2,
                    line: 1,
                    old_snippet: "ab".into(),
                    new_snippet: "xy".into(),
                    kind: EditKind::Reference,
                },
                PlannedEdit {
                    path: p,
                    start_byte: 1,
                    end_byte: 3,
                    line: 1,
                    old_snippet: "bc".into(),
                    new_snippet: "yz".into(),
                    kind: EditKind::Reference,
                },
            ],
        };
        let err = check_for_overlaps(&plan).expect_err("expected overlap error");
        assert!(err.contains("overlapping edits"));
    }

    #[test]
    fn read_original_snapshots_marks_missing_files() {
        let p = PathBuf::from("/tmp/kiss_missing_snapshot_test_path");
        let snaps = read_original_snapshots(std::slice::from_ref(&p)).unwrap();
        assert!(!snaps.get(&p).expect("snapshot entry").existed);
    }

    #[test]
    fn group_edits_by_path_collects_edits_per_file() {
        let a = PathBuf::from("a.txt");
        let b = PathBuf::from("b.txt");
        let e1 = PlannedEdit {
            path: a.clone(),
            start_byte: 0,
            end_byte: 1,
            line: 1,
            old_snippet: "a".into(),
            new_snippet: "b".into(),
            kind: EditKind::Reference,
        };
        let e2 = PlannedEdit {
            path: b.clone(),
            start_byte: 0,
            end_byte: 1,
            line: 1,
            old_snippet: "a".into(),
            new_snippet: "b".into(),
            kind: EditKind::Reference,
        };
        let plan = MvPlan {
            files: vec![a.clone(), b.clone()],
            edits: vec![e1, e2],
        };
        let grouped = group_edits_by_path(&plan);
        assert_eq!(grouped.get(&a).map(Vec::len), Some(1));
        assert_eq!(grouped.get(&b).map(Vec::len), Some(1));
    }

    #[test]
    fn apply_all_file_edits_skips_empty_edit_list_without_creating_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("absent.txt");
        assert!(!p.exists());
        let mut per_file: BTreeMap<PathBuf, Vec<&PlannedEdit>> = BTreeMap::new();
        per_file.insert(p.clone(), vec![]);
        let mut originals = BTreeMap::new();
        originals.insert(
            p.clone(),
            Snapshot {
                existed: false,
                content: String::new(),
            },
        );
        apply_all_file_edits(&originals, &mut per_file).unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn apply_plan_transactional_success_writes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("patchme.txt");
        fs::write(&p, "ab").unwrap();
        let plan = MvPlan {
            files: vec![p.clone()],
            edits: vec![PlannedEdit {
                path: p.clone(),
                start_byte: 0,
                end_byte: 1,
                line: 1,
                old_snippet: "a".into(),
                new_snippet: "z".into(),
                kind: EditKind::Reference,
            }],
        };
        apply_plan_transactional(&plan).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "zb");
    }

    #[test]
    fn apply_plan_transactional_rolls_back_partial_writes_and_restores_new_file_state() {
        let tmp = tempfile::tempdir().unwrap();
        let existing = tmp.path().join("a.txt");
        let missing = tmp.path().join("missing.txt");
        fs::write(&existing, "ab").unwrap();
        assert!(!missing.exists());

        let write_existing = PlannedEdit {
            path: existing.clone(),
            start_byte: 0,
            end_byte: 1,
            line: 1,
            old_snippet: "a".into(),
            new_snippet: "z".into(),
            kind: EditKind::Reference,
        };
        let mut invalid = PlannedEdit {
            path: missing.clone(),
            start_byte: 0,
            end_byte: 1,
            line: 1,
            old_snippet: String::new(),
            new_snippet: String::new(),
            kind: EditKind::Definition,
        };
        // force a failure on a second file so rollback logic must restore prior state
        invalid.start_byte = 0;
        invalid.end_byte = 2;
        let plan = MvPlan {
            files: vec![existing.clone(), missing.clone()],
            edits: vec![write_existing, invalid],
        };
        let err = apply_plan_transactional(&plan).expect_err("expected apply failure");
        assert!(err.contains("invalid edit range"));
        assert_eq!(fs::read_to_string(&existing).unwrap(), "ab");
        assert!(!missing.exists());
    }

    #[test]
    fn rollback_restores_snapshotted_file_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("rollback.txt");
        fs::write(&p, "modified").unwrap();
        let mut originals = BTreeMap::new();
        originals.insert(
            p.clone(),
            Snapshot {
                existed: true,
                content: "original".into(),
            },
        );
        rollback(&originals).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "original");
    }
}
