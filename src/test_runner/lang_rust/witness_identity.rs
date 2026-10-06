use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use crate::test_runner::lang_iface::ExecutionWitness;

pub(crate) fn rust_identity_digest_from_batch(identity: &RustCoverageBatchIdentity) -> String {
    format!(
        "rs:{}:{}:{}",
        identity.input_digest,
        identity.generation_fingerprint,
        identity.selection_context_fingerprint
    )
}

fn rust_witness_row_reportable(witness: &ExecutionWitness, i: usize) -> bool {
    witness
        .statuses
        .get(i)
        .and_then(|status| status.to_test_status())
        .is_some()
        && witness.durations_ns.get(i).copied().flatten().is_some()
}

pub(crate) fn rust_witness_overlap(planned: &[String], witness: &ExecutionWitness) -> Vec<String> {
    planned
        .iter()
        .filter(|selector| {
            witness
                .selectors
                .iter()
                .position(|stored| stored == *selector)
                .is_some_and(|i| rust_witness_row_reportable(witness, i))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_keeps_only_reportable_rows() {
        use crate::test_runner::lang_iface::WitnessStatus;
        let witness = ExecutionWitness {
            language: "rust".into(),
            identity_digest: "rs:input:old-gen:old-sel".into(),
            selectors: vec!["pass".into(), "fail".into(), "unresolved".into()],
            statuses: vec![
                WitnessStatus::Passed,
                WitnessStatus::Failed,
                WitnessStatus::Unresolved,
            ],
            durations_ns: vec![Some(1), Some(2), None],
            covered_lines: Default::default(),
            complete: false,
            generation_id: "g".into(),
            raw_statuses: vec![
                WitnessStatus::Passed,
                WitnessStatus::Failed,
                WitnessStatus::Unresolved,
            ],
        };
        let planned: Vec<String> = vec![
            "pass".into(),
            "fail".into(),
            "unresolved".into(),
            "extra".into(),
        ];
        assert_eq!(
            rust_witness_overlap(&planned, &witness),
            vec!["pass".to_string(), "fail".to_string()]
        );
    }
}
