
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use rslip::{CacheStatus, LineCoverage, RslipOutcome};

use super::types::{
    CoveredLinesMap, LineIndexMap, SelectorCoverageMap, SelectorTimingRecord,
    TimingCacheDisposition,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct PopulationEvidence {
    pub(crate) coverage: CoveredLinesMap,
    pub(crate) selector_coverage: SelectorCoverageMap,
    pub(crate) line_index: LineIndexMap,
    pub(crate) timings: Vec<SelectorTimingRecord>,
    pub(crate) line_refs: BTreeMap<(String, u32), u32>,
    pub(crate) complete: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SelectorEvidence {
    pub(crate) selector: String,
    pub(crate) raw_status: TestStatus,
    pub(crate) effective_status: TestStatus,
    pub(crate) duration: Option<Duration>,
    pub(crate) cache_disposition: TimingCacheDisposition,
    pub(crate) reason: Option<String>,
    pub(crate) coverage: CoveredLinesMap,
}

impl PopulationEvidence {
    pub(crate) fn from_ordered_selectors(selectors: &[String]) -> Self {
        let mut evidence = Self::default();
        evidence.timings.reserve(selectors.len());
        for selector in selectors {
            evidence.timings.push(unresolved_timing(selector));
        }
        evidence.complete = false;
        evidence
    }

    pub(crate) fn recompute_complete(&mut self) {

        self.complete = self
            .timings
            .iter()
            .all(|row| row.effective_status == "passed");
    }

    pub(crate) fn absorb_selector(&mut self, item: SelectorEvidence) {
        let Some(slot) = self
            .timings
            .iter_mut()
            .find(|row| row.selector == item.selector)
        else {
            return;
        };
        *slot = SelectorTimingRecord {
            selector: item.selector.clone(),
            raw_status: status_label(item.raw_status),
            effective_status: status_label(item.effective_status),
            duration_ns: item.duration.map(|d| d.as_nanos() as u64),
            cache_disposition: item.cache_disposition,
            reason: item.reason,
        };
        replace_selector_coverage(self, &item.selector, item.coverage);
        self.recompute_complete();
    }
}

pub(crate) fn selector_evidence_from_outcome(
    repo_root: &Path,
    outcome: &RslipOutcome,
    effective_status: TestStatus,
    reason: Option<String>,
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
) -> SelectorEvidence {
    SelectorEvidence {
        selector: outcome.nodeid.clone(),
        raw_status: outcome.status,
        effective_status,
        duration: measured_duration(outcome),
        cache_disposition: cache_disposition(outcome.cache_status),
        reason,
        coverage: filter_coverage(repo_root, &outcome.coverage, is_indexable),
    }
}

fn measured_duration(outcome: &RslipOutcome) -> Option<Duration> {
    if outcome.duration.is_zero() && outcome.cache_status == CacheStatus::Hit {

        return None;
    }
    Some(outcome.duration)
}

fn cache_disposition(status: CacheStatus) -> TimingCacheDisposition {
    match status {
        CacheStatus::Hit => TimingCacheDisposition::Hit,
        CacheStatus::MissStored => TimingCacheDisposition::MissStored,
    }
}

pub(crate) fn status_label(status: TestStatus) -> String {
    match status {
        TestStatus::Passed => "passed".to_string(),
        TestStatus::Failed => "failed".to_string(),
        TestStatus::TimedOut => "timed_out".to_string(),
    }
}

fn unresolved_timing(selector: &str) -> SelectorTimingRecord {
    SelectorTimingRecord {
        selector: selector.to_string(),
        raw_status: "unresolved".to_string(),
        effective_status: "unresolved".to_string(),
        duration_ns: None,
        cache_disposition: TimingCacheDisposition::Unknown,
        reason: Some("missing outcome".to_string()),
    }
}

fn filter_coverage(
    repo_root: &Path,
    coverage: &LineCoverage,
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
) -> CoveredLinesMap {
    let mut out = CoveredLinesMap::new();
    for (file, lines) in &coverage.files {
        let path = Path::new(file);
        if !is_indexable(path, repo_root) {
            continue;
        }
        let Some(rel) =
            crate::test_runner::python_coverage_index::repo_relative_coverage_file(repo_root, file)
        else {
            continue;
        };
        out.entry(rel).or_default().extend(lines.iter().copied());
    }
    out
}

fn replace_selector_coverage(
    evidence: &mut PopulationEvidence,
    selector: &str,
    coverage: CoveredLinesMap,
) {
    if let Some(old) = evidence.selector_coverage.remove(selector) {
        remove_coverage_contribution(evidence, selector, &old);
    }
    add_coverage_contribution(evidence, selector, &coverage);
    evidence
        .selector_coverage
        .insert(selector.to_string(), coverage);
}

fn remove_coverage_contribution(
    evidence: &mut PopulationEvidence,
    selector: &str,
    coverage: &CoveredLinesMap,
) {
    for (file, lines) in coverage {
        for &line in lines {
            decrement_line_ref(evidence, file, line);
            remove_line_index_selector(evidence, file, line, selector);
        }
    }
}

fn decrement_line_ref(evidence: &mut PopulationEvidence, file: &str, line: u32) {
    let key = (file.to_string(), line);
    let Some(count) = evidence.line_refs.get_mut(&key) else {
        return;
    };
    *count = count.saturating_sub(1);
    if *count > 0 {
        return;
    }
    evidence.line_refs.remove(&key);
    let Some(file_lines) = evidence.coverage.get_mut(file) else {
        return;
    };
    file_lines.remove(&line);
    if file_lines.is_empty() {
        evidence.coverage.remove(file);
    }
}

fn remove_line_index_selector(
    evidence: &mut PopulationEvidence,
    file: &str,
    line: u32,
    selector: &str,
) {
    let Some(file_index) = evidence.line_index.get_mut(file) else {
        return;
    };
    if let Some(selectors) = file_index.get_mut(&line) {
        selectors.remove(selector);
        if selectors.is_empty() {
            file_index.remove(&line);
        }
    }
    if file_index.is_empty() {
        evidence.line_index.remove(file);
    }
}

fn add_coverage_contribution(
    evidence: &mut PopulationEvidence,
    selector: &str,
    coverage: &CoveredLinesMap,
) {
    for (file, lines) in coverage {
        for &line in lines {
            let key = (file.clone(), line);
            *evidence.line_refs.entry(key).or_insert(0) += 1;
            evidence
                .coverage
                .entry(file.clone())
                .or_default()
                .insert(line);
            evidence
                .line_index
                .entry(file.clone())
                .or_default()
                .entry(line)
                .or_default()
                .insert(selector.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::time::Duration;

    #[test]
    fn absorb_replaces_unique_lines_and_keeps_shared() {
        let mut evidence = PopulationEvidence::from_ordered_selectors(&[
            "a".to_string(),
            "b".to_string(),
        ]);
        let shared = BTreeMap::from([("f.py".to_string(), BTreeSet::from([1, 2]))]);
        evidence.absorb_selector(SelectorEvidence {
            selector: "a".to_string(),
            raw_status: TestStatus::Passed,
            effective_status: TestStatus::Passed,
            duration: Some(Duration::from_millis(1)),
            cache_disposition: TimingCacheDisposition::MissStored,
            reason: None,
            coverage: shared.clone(),
        });
        evidence.absorb_selector(SelectorEvidence {
            selector: "b".to_string(),
            raw_status: TestStatus::Passed,
            effective_status: TestStatus::Passed,
            duration: Some(Duration::from_millis(1)),
            cache_disposition: TimingCacheDisposition::MissStored,
            reason: None,
            coverage: BTreeMap::from([("f.py".to_string(), BTreeSet::from([2, 3]))]),
        });
        evidence.absorb_selector(SelectorEvidence {
            selector: "a".to_string(),
            raw_status: TestStatus::Passed,
            effective_status: TestStatus::Passed,
            duration: Some(Duration::from_millis(2)),
            cache_disposition: TimingCacheDisposition::MissStored,
            reason: None,
            coverage: BTreeMap::from([("f.py".to_string(), BTreeSet::from([4]))]),
        });
        assert_eq!(
            evidence.coverage.get("f.py"),
            Some(&BTreeSet::from([2, 3, 4]))
        );
        assert!(evidence.complete);
    }
}
