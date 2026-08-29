use super::super::CheckAggregateRepairDecision;
use super::{aggregate_prior, aggregate_prior_with_maps};

#[test]
fn replacement_binary_stays_rerun_when_digest_changes() {
    let selectors = vec!["pkg::bin$alpha".to_string(), "pkg::bin$beta".to_string()];
    let prior = aggregate_prior_with_maps(
        &selectors,
        &[("bin-a", "old"), ("bin-b", "stable")],
        &[
            (selectors[0].as_str(), vec!["bin-a"]),
            (selectors[1].as_str(), vec!["bin-b"]),
        ],
    );
    let maps = std::collections::BTreeMap::from([
        (selectors[0].clone(), vec!["bin-a".to_string()]),
        (selectors[1].clone(), vec!["bin-b".to_string()]),
    ]);
    let binaries = vec![
        super::test_binary("bin-a", "new"),
        super::test_binary("bin-b", "stable"),
    ];
    match super::super::classify_check_aggregate_repair(&selectors, &prior, &maps, &binaries) {
        CheckAggregateRepairDecision::Rerun {
            rerun_selectors,
            retained_binary_line_maps,
            ..
        } => {
            assert_eq!(rerun_selectors, vec![selectors[0].clone()]);
            assert!(retained_binary_line_maps.contains_key("bin-b"));
            assert!(!retained_binary_line_maps.contains_key("bin-a"));
        }
        other => panic!("expected Rerun, got {other:?}"),
    }
}

#[test]
fn identity_only_keeps_digest_matched_maps() {
    let selectors = vec!["a".into()];
    let prior = aggregate_prior(&selectors, &[("bin-a", "digest-a")]);
    let maps =
        std::collections::BTreeMap::from([(selectors[0].clone(), vec!["bin-a".to_string()])]);
    let binaries = vec![super::test_binary("bin-a", "digest-a")];
    let out = super::super::classify_check_aggregate_repair(&selectors, &prior, &maps, &binaries);
    assert!(matches!(
        out,
        CheckAggregateRepairDecision::IdentityOnly { .. }
    ));
}
