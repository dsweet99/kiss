use std::collections::{BTreeMap, BTreeSet};

const LCS_LINE_LIMIT: usize = 8_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFileDiff {
    pub remapped: BTreeMap<u32, u32>,
    pub invalidated_old_lines: BTreeSet<u32>,
    pub ambiguous: bool,
}

pub fn diff_file_lines(old: &str, new: &str) -> SourceFileDiff {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    diff_eq_slices(&old_lines, &new_lines)
}

pub fn remap_line_set(
    old_hashes: &[u64],
    new_hashes: &[u64],
    old_lines: &BTreeSet<u32>,
) -> BTreeSet<u32> {
    let diff = diff_eq_slices(old_hashes, new_hashes);
    if diff.ambiguous {
        return BTreeSet::new();
    }
    old_lines
        .iter()
        .filter_map(|line| diff.remapped.get(line).copied())
        .collect()
}

pub fn diff_eq_slices<T: Eq>(old_lines: &[T], new_lines: &[T]) -> SourceFileDiff {
    if old_lines.len() > LCS_LINE_LIMIT || new_lines.len() > LCS_LINE_LIMIT {
        return whole_file_invalid(old_lines.len());
    }
    if old_lines == new_lines {
        let remapped = (1..=old_lines.len() as u32)
            .map(|line| (line, line))
            .collect();
        return SourceFileDiff {
            remapped,
            invalidated_old_lines: BTreeSet::new(),
            ambiguous: false,
        };
    }
    let pairs = lcs_index_pairs(old_lines, new_lines);
    let mut remapped = BTreeMap::new();
    let mut matched_old = vec![false; old_lines.len()];
    for (old_i, new_i) in pairs {
        remapped.insert((old_i + 1) as u32, (new_i + 1) as u32);
        matched_old[old_i] = true;
    }
    let invalidated_old_lines = matched_old
        .iter()
        .enumerate()
        .filter(|(_, matched)| !**matched)
        .map(|(i, _)| (i + 1) as u32)
        .collect();
    SourceFileDiff {
        remapped,
        invalidated_old_lines,
        ambiguous: false,
    }
}

fn whole_file_invalid(old_len: usize) -> SourceFileDiff {
    SourceFileDiff {
        remapped: BTreeMap::new(),
        invalidated_old_lines: (1..=old_len as u32).collect(),
        ambiguous: true,
    }
}

fn lcs_index_pairs<T: Eq>(old_lines: &[T], new_lines: &[T]) -> Vec<(usize, usize)> {
    let n = old_lines.len();
    let m = new_lines.len();
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old_lines[i] == new_lines[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < n && j < m {
        if old_lines[i] == new_lines[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_files_remap_every_line() {
        let diff = diff_file_lines("a\nb\n", "a\nb\n");
        assert_eq!(diff.remapped.get(&1), Some(&1));
        assert_eq!(diff.remapped.get(&2), Some(&2));
        assert!(diff.invalidated_old_lines.is_empty());
        assert!(!diff.ambiguous);
    }

    #[test]
    fn inserted_line_shifts_later_coordinates() {
        let diff = diff_file_lines("a\nc\n", "a\nb\nc\n");
        assert_eq!(diff.remapped.get(&1), Some(&1));
        assert_eq!(diff.remapped.get(&2), Some(&3));
        assert!(diff.invalidated_old_lines.is_empty());
    }

    #[test]
    fn changed_line_is_invalidated() {
        let diff = diff_file_lines("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(diff.remapped.get(&1), Some(&1));
        assert_eq!(diff.remapped.get(&3), Some(&3));
        assert!(diff.invalidated_old_lines.contains(&2));
        assert!(!diff.remapped.contains_key(&2));
    }

    #[test]
    fn hash_slice_insert_keeps_old_lines() {
        let diff = diff_eq_slices(&[1u64, 2], &[1u64, 9, 2]);
        assert!(diff.invalidated_old_lines.is_empty());
        assert_eq!(diff.remapped.get(&2), Some(&3));
        let remapped = remap_line_set(&[1u64, 2], &[1u64, 9, 2], &BTreeSet::from([1, 2]));
        assert_eq!(remapped, BTreeSet::from([1, 3]));
    }
}
