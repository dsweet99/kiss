use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::test_runner::lang_iface::ExecutionWitness;

type DurationPair = (String, Duration);
type DurationPairsMemo = Option<(PathBuf, Vec<DurationPair>)>;

thread_local! {
    static RUST_DURATION_PAIRS_MEMO: RefCell<DurationPairsMemo> = const { RefCell::new(None) };
}

pub(super) fn load_rust_duration_pairs(repo_root: &Path) -> Option<Vec<DurationPair>> {
    let repo_key = repo_root.to_path_buf();
    if let Some(cached) = RUST_DURATION_PAIRS_MEMO.with(|memo| {
        memo.borrow()
            .as_ref()
            .filter(|(root, _)| root == &repo_key)
            .map(|(_, pairs)| pairs.clone())
    }) {
        return Some(cached);
    }
    let witness =
        crate::test_runner::execution_witness::try_load_rust_execution_witness(repo_root, &[])
            .ok()?;
    let pairs = duration_pairs(&witness)?;
    RUST_DURATION_PAIRS_MEMO.with(|memo| {
        *memo.borrow_mut() = Some((repo_key, pairs.clone()));
    });
    Some(pairs)
}

pub(super) fn duration_pairs(witness: &ExecutionWitness) -> Option<Vec<DurationPair>> {
    if !witness.complete
        || witness.selectors.is_empty()
        || witness.durations_ns.len() != witness.selectors.len()
        || witness
            .statuses
            .iter()
            .any(|status| *status != crate::test_runner::lang_iface::WitnessStatus::Passed)
    {
        return None;
    }
    witness
        .selectors
        .iter()
        .zip(witness.durations_ns.iter())
        .map(|(selector, &ns)| Some((selector.clone(), Duration::from_nanos(ns?))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::execution_witness::WitnessStatus;

    #[test]
    fn absent_records_give_no_durations() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_rust_duration_pairs(tmp.path()).is_none());
    }

    #[test]
    fn passing_records_give_their_durations() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_runner::lang_rust::test_records::store(
            tmp.path(),
            &[("tests/a.rs::test_1".to_string(), WitnessStatus::Passed)],
        );
        let pairs = load_rust_duration_pairs(tmp.path()).unwrap();
        assert_eq!(
            pairs,
            [("tests/a.rs::test_1".to_string(), Duration::from_nanos(1))]
        );
    }

    #[test]
    fn a_failing_record_withholds_durations() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_runner::lang_rust::test_records::store(
            tmp.path(),
            &[
                ("a".to_string(), WitnessStatus::Passed),
                ("b".to_string(), WitnessStatus::Failed),
            ],
        );
        let witness = crate::test_runner::lang_rust::test_records::load(tmp.path()).unwrap();
        assert!(duration_pairs(&witness).is_none());
    }
}
