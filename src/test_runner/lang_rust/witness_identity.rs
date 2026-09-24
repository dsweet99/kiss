use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use crate::test_runner::lang_iface::{
    EnsureRequest, ExecutionWitness, identity_covers, miss_selectors_for_repair,
};

pub(crate) fn rust_identity_digest_from_batch(identity: &RustCoverageBatchIdentity) -> String {
    format!(
        "rs:{}:{}:{}",
        identity.input_digest,
        identity.generation_fingerprint,
        identity.selection_context_fingerprint
    )
}

pub(crate) fn rust_source_identity_covers(witness_digest: &str, current: &str) -> bool {
    if identity_covers(witness_digest, current) {
        return true;
    }
    let witness_input = rust_identity_input_segment(witness_digest);
    witness_input.is_some() && witness_input == rust_identity_input_segment(current)
}

fn rust_identity_input_segment(digest: &str) -> Option<&str> {
    digest
        .strip_prefix("rs:")?
        .split(':')
        .next()
        .filter(|segment| !segment.is_empty())
}

pub(crate) fn rust_live_miss_selectors(
    request: &EnsureRequest,
    planned: &[String],
    identity: &str,
    witness: Option<&ExecutionWitness>,
) -> Vec<String> {
    if request.force {
        return planned.to_vec();
    }
    let Some(witness) = witness else {
        return planned.to_vec();
    };
    if witness.identity_digest.starts_with("rs:")
        && rust_source_identity_covers(&witness.identity_digest, identity)
    {
        return Vec::new();
    }
    miss_selectors_for_repair(request.mode, planned, identity, Some(witness), false)
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
    fn source_cover_matches_input_and_rejects_empty_prefix() {
        assert!(rust_source_identity_covers(
            "rs:input:gen-a:sel-a",
            "rs:input:gen-b:sel-b"
        ));
        assert!(!rust_source_identity_covers(
            "rs:input:gen-a:sel-a",
            "rs:other:gen-a:sel-a"
        ));
        assert!(!rust_source_identity_covers("py:input", "rs:input:g:s"));
        assert!(!rust_source_identity_covers("rs:", "rs:input:g:s"));
    }

    #[test]
    fn live_misses_empty_on_source_stable_fail_and_extra() {
        use crate::test_runner::lang_iface::{
            AcceptMode, EnsureRequest, WitnessScope, WitnessStatus,
        };
        let witness = ExecutionWitness {
            language: "rust".into(),
            scope: WitnessScope::Full,
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
        let req = EnsureRequest {
            repo_root: std::path::PathBuf::from("."),
            mode: AcceptMode::All,
            lang_filter: Some(kiss::Language::Rust),
            ignore: vec![],
            force: false,
            force_selectors: Vec::new(),
            jobs: 1,
            gate: kiss::GateConfig::default(),
            extras: crate::test_runner::language_keyed::LanguageKeyed {
                python: vec![],
                rust: vec![],
            },
            planned: crate::test_runner::language_keyed::LanguageKeyed {
                python: vec![],
                rust: vec![
                    "pass".into(),
                    "fail".into(),
                    "unresolved".into(),
                    "extra".into(),
                ],
            },
        };
        assert!(
            rust_live_miss_selectors(
                &req,
                &req.planned.rust,
                "rs:input:new-gen:new-sel",
                Some(&witness),
            )
            .is_empty()
        );
        let mut other = req;
        other.force = true;
        assert_eq!(
            rust_live_miss_selectors(
                &other,
                &other.planned.rust,
                "rs:input:new-gen:new-sel",
                Some(&witness),
            )
            .len(),
            4
        );
        assert_eq!(
            rust_witness_overlap(&other.planned.rust, &witness),
            vec!["pass".to_string(), "fail".to_string()]
        );
    }
}
