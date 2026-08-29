use super::witness::{ExecutionWitness, WitnessStatus};

pub(super) fn reusable_without_rerun(witness: &ExecutionWitness, i: usize) -> bool {
    matches!(witness.statuses.get(i), Some(WitnessStatus::Passed))
        || gate_violation_from_raw_pass(witness, i)
}

pub(super) fn gate_violation_from_raw_pass(witness: &ExecutionWitness, i: usize) -> bool {
    if witness.durations_ns.get(i).copied().flatten().is_none() {
        return false;
    }
    let effective = match witness.statuses.get(i) {
        Some(status) => *status,
        None => return false,
    };
    let raw = witness.raw_statuses.get(i).copied().unwrap_or(effective);
    raw == WitnessStatus::Passed
        && matches!(effective, WitnessStatus::TimedOut | WitnessStatus::Failed)
}

pub(crate) fn miss_is_warm_skippable(witness: &ExecutionWitness, i: usize) -> bool {
    if witness.durations_ns.get(i).copied().flatten().is_none() {
        return false;
    }
    let effective = witness.statuses[i];
    let raw = witness.raw_statuses.get(i).copied().unwrap_or(effective);
    match raw {
        WitnessStatus::Passed => {
            matches!(effective, WitnessStatus::TimedOut | WitnessStatus::Failed)
        }
        WitnessStatus::Unresolved => true,
        WitnessStatus::TimedOut | WitnessStatus::Failed => false,
    }
}
