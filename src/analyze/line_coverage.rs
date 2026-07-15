use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use kiss::check_universe_cache::CachedLineCoverageRecord;

use crate::analyze::coverage_gate::is_coverage_gate_file;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeCoverageSnapshot {
    pub(crate) identity: String,
    pub(crate) covered_lines: BTreeMap<String, BTreeSet<u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LineCoverageRecord {
    pub(crate) file: PathBuf,
    pub(crate) total_lines: usize,
    pub(crate) covered_lines: usize,
    pub(crate) percent: usize,
    pub(crate) first_uncovered_line: Option<usize>,
}

pub(crate) fn compute_line_coverage_records(
    repo_root: &Path,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    snapshot: &RuntimeCoverageSnapshot,
) -> Vec<LineCoverageRecord> {
    let mut records = py_files
        .iter()
        .chain(rs_files)
        .filter(|path| is_coverage_gate_file(path))
        .map(|path| compute_file_line_coverage(repo_root, path, snapshot))
        .collect::<Vec<_>>();
    records.sort_by(|a, b| a.file.cmp(&b.file));
    records
}

pub(crate) fn compute_file_line_coverage(
    repo_root: &Path,
    file: &Path,
    snapshot: &RuntimeCoverageSnapshot,
) -> LineCoverageRecord {
    let total_lines = physical_line_count(file);
    let rel = repo_relative_key(repo_root, file);
    let covered = rel
        .as_ref()
        .and_then(|key| snapshot.covered_lines.get(key))
        .map_or(0, |lines| {
            lines
                .iter()
                .filter(|line| **line > 0 && (**line as usize) <= total_lines)
                .collect::<BTreeSet<_>>()
                .len()
        });
    let first_uncovered_line = (1..=total_lines).find(|line| {
        rel.as_ref()
            .and_then(|key| snapshot.covered_lines.get(key))
            .is_none_or(|lines| !lines.contains(&(*line as u32)))
    });
    let percent = if total_lines == 0 {
        100
    } else {
        percentage(covered, total_lines)
    };
    LineCoverageRecord {
        file: file.to_path_buf(),
        total_lines,
        covered_lines: covered,
        percent,
        first_uncovered_line,
    }
}

pub(crate) fn cached_line_records(records: &[LineCoverageRecord]) -> Vec<CachedLineCoverageRecord> {
    records
        .iter()
        .map(|record| CachedLineCoverageRecord {
            file: record.file.to_string_lossy().to_string(),
            total_lines: record.total_lines,
            covered_lines: record.covered_lines,
            percent: record.percent,
            first_uncovered_line: record.first_uncovered_line,
        })
        .collect()
}

pub(crate) fn line_records_from_cache(
    records: &[CachedLineCoverageRecord],
) -> Vec<LineCoverageRecord> {
    records
        .iter()
        .map(|record| LineCoverageRecord {
            file: PathBuf::from(&record.file),
            total_lines: record.total_lines,
            covered_lines: record.covered_lines,
            percent: record.percent,
            first_uncovered_line: record.first_uncovered_line,
        })
        .collect()
}

fn physical_line_count(file: &Path) -> usize {
    let Ok(contents) = fs::read_to_string(file) else {
        return 0;
    };
    if contents.is_empty() {
        0
    } else {
        contents.lines().count()
    }
}

fn repo_relative_key(repo_root: &Path, file: &Path) -> Option<String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    canonical
        .strip_prefix(root)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn percentage(covered: usize, total: usize) -> usize {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    {
        ((covered as f64 / total as f64) * 100.0).round() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_physical_line_percent_and_first_uncovered() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("src").join("app.py");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "a\nb\nc\n").unwrap();
        let snapshot = RuntimeCoverageSnapshot {
            identity: "id".to_string(),
            covered_lines: BTreeMap::from([(
                "src/app.py".to_string(),
                BTreeSet::from([0, 1, 3, 9]),
            )]),
        };

        let record = compute_file_line_coverage(tmp.path(), &file, &snapshot);

        assert_eq!(record.total_lines, 3);
        assert_eq!(record.covered_lines, 2);
        assert_eq!(record.percent, 67);
        assert_eq!(record.first_uncovered_line, Some(2));
    }

    #[test]
    fn empty_file_is_fully_covered() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("empty.py");
        std::fs::write(&file, "").unwrap();
        let snapshot = RuntimeCoverageSnapshot {
            identity: "id".to_string(),
            covered_lines: BTreeMap::new(),
        };

        let record = compute_file_line_coverage(tmp.path(), &file, &snapshot);

        assert_eq!(record.percent, 100);
        assert_eq!(record.first_uncovered_line, None);
    }
}
