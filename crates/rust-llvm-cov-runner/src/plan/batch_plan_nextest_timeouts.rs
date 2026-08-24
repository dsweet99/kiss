use std::collections::BTreeMap;

use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::plan::batch_plan_nextest_config::{nextest_filter_string, toml_basic_string};

pub(crate) fn append_slow_timeout_toml(out: &mut String, req: &RustCoverageBatchRequest) {
    if req.selector_timeout_millis.is_empty() {
        return;
    }
    if let Some(period) = uniform_check_aggregate_timeout_period(req) {
        out.push_str(&format!(
            "slow-timeout = {{ period = {period}, terminate-after = 1 }}\n",
            period = toml_basic_string(&period)
        ));
        return;
    }

    let exact = true;
    let mut by_period: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for selector in &req.logical_selectors {
        let Some(&millis) = req.selector_timeout_millis.get(selector) else {
            continue;
        };
        if millis == 0 {
            continue;
        }
        by_period
            .entry(format_nextest_period_millis(millis))
            .or_default()
            .push(selector.as_str());
    }
    for (period, selectors) in by_period {
        let filter = selectors
            .into_iter()
            .map(|selector| format!("test({})", nextest_filter_string(selector, exact)))
            .collect::<Vec<_>>()
            .join(" | ");
        out.push_str("\n[[profile.kiss.overrides]]\n");
        out.push_str(&format!("filter = {}\n", toml_basic_string(&filter)));
        out.push_str(&format!(
            "slow-timeout = {{ period = {period}, terminate-after = 1 }}\n",
            period = toml_basic_string(&period)
        ));
    }
}

fn format_nextest_period_millis(millis: u64) -> String {
    let millis = millis.max(1);
    if millis.is_multiple_of(1000) {
        format!("{}s", millis / 1000)
    } else {
        format!("{millis}ms")
    }
}

fn uniform_check_aggregate_timeout_period(req: &RustCoverageBatchRequest) -> Option<String> {
    if !matches!(
        req.coverage_output_mode,
        crate::plan::batch_plan::CoverageOutputMode::CheckAggregate { .. }
    ) || req.logical_selectors.len() <= 64
    {
        return None;
    }
    let mut period = None;
    for selector in &req.logical_selectors {
        let millis = *req.selector_timeout_millis.get(selector)?;
        if millis == 0 {
            continue;
        }
        let formatted = format_nextest_period_millis(millis);
        match &period {
            None => period = Some(formatted),
            Some(existing) if existing == &formatted => {}
            Some(_) => return None,
        }
    }
    period
}

#[cfg(test)]
mod tests {
    use super::{append_slow_timeout_toml, format_nextest_period_millis};
    use std::collections::BTreeMap;

    #[test]
    fn period_formatting_uses_seconds_or_millis() {
        assert_eq!(format_nextest_period_millis(2000), "2s");
        assert_eq!(format_nextest_period_millis(500), "500ms");
        assert_eq!(format_nextest_period_millis(1250), "1250ms");
    }

    #[test]
    fn overrides_group_selectors_sharing_a_limit() {
        let mut req = crate::plan::batch_plan::RustCoverageBatchRequest::witness();
        req.test_args.clear();
        req.logical_selectors = vec!["alpha".into(), "beta".into(), "gamma".into()];
        req.selector_timeout_millis = BTreeMap::from([
            ("alpha".into(), 2000),
            ("beta".into(), 2000),
            ("gamma".into(), 500),
        ]);
        let mut toml = String::from("[profile.kiss]\n");
        append_slow_timeout_toml(&mut toml, &req);
        assert!(toml.contains("terminate-after = 1"), "toml={toml}");
        assert!(toml.contains("period = \"2s\""), "toml={toml}");
        assert!(toml.contains("period = \"500ms\""), "toml={toml}");
        assert!(
            toml.contains("alpha$/") && toml.contains("(^|"),
            "timeout filters must be exact/anchored, toml={toml}"
        );
        assert!(
            toml.contains("beta$/") && toml.contains("gamma$/"),
            "toml={toml}"
        );
        assert!(
            !toml.contains("filter = \"test(/alpha/)\""),
            "unanchored timeout filter must not appear, toml={toml}"
        );
    }

    #[test]
    fn large_check_aggregate_uniform_timeout_uses_profile_slow_timeout() {
        let mut req = crate::plan::batch_plan::RustCoverageBatchRequest::witness();
        req.coverage_output_mode = crate::plan::batch_plan::CoverageOutputMode::CheckAggregate {
            publication_binary_ids: None,
            repair_publication: None,
        };
        req.logical_selectors = (0..80).map(|i| format!("t{i}")).collect();
        req.selector_timeout_millis = req
            .logical_selectors
            .iter()
            .cloned()
            .map(|selector| (selector, 15_000))
            .collect();
        let mut toml = String::from("[profile.kiss]\n");
        append_slow_timeout_toml(&mut toml, &req);
        assert!(
            toml.contains("slow-timeout = { period = \"15s\", terminate-after = 1 }"),
            "toml={toml}"
        );
        assert!(
            !toml.contains("[[profile.kiss.overrides]]"),
            "uniform CheckAggregate timeout must not emit per-selector overrides, toml={toml}"
        );
        assert!(!toml.contains("test(t0"), "toml={toml}");
    }
}
