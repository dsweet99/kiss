//! Per-selector nextest `slow-timeout` overrides for kiss unit-test limits.

use std::collections::BTreeMap;

use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::plan::batch_plan_nextest_config::{nextest_filter_string, toml_basic_string};

/// Append profile-level / per-selector `slow-timeout` rules (`terminate-after = 1`).
pub(crate) fn append_slow_timeout_toml(out: &mut String, req: &RustCoverageBatchRequest) {
    if req.selector_timeout_millis.is_empty() {
        return;
    }
    // Timeout overrides always use exact/anchored filters so one selector's
    // limit cannot kill substring-overlapping tests.
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
}
