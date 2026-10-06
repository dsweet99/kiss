use std::time::Duration;

use super::merge_exit_codes;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectorExecutionSummary {
    pub(crate) exit_code: i32,
    pub(crate) total: usize,
    pub(crate) cache_hits: usize,
    pub(crate) cache_misses: usize,
    pub(crate) cache_miss_selectors: Vec<String>,
    pub(crate) cache_unstored: usize,
    pub(crate) cache_unstored_selectors: Vec<String>,
    pub(crate) failed: usize,
    pub(crate) failed_selectors: Vec<String>,
    pub(crate) timed_out_selectors: Vec<String>,
    pub(crate) selector_durations_ns: std::collections::BTreeMap<String, u64>,
    pub(crate) raw_statuses: std::collections::BTreeMap<String, kiss::rpytest_runner::TestStatus>,
    pub(crate) max_passing_run_duration: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectorCacheRecord {
    Hit,
    MissStored,
    MissUnstored,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectorExecutionRecord {
    pub(crate) selector: String,
    pub(crate) status: kiss::rpytest_runner::TestStatus,
    pub(crate) raw_status: Option<kiss::rpytest_runner::TestStatus>,
    pub(crate) cache_record: SelectorCacheRecord,
    pub(crate) exit_code: Option<i32>,
    pub(crate) duration: Duration,
}

impl SelectorExecutionSummary {
    pub(crate) fn with_records(
        mut self,
        records: impl IntoIterator<Item = SelectorExecutionRecord>,
    ) -> Self {
        for record in records {
            self.record(record);
        }
        self
    }

    pub(crate) fn record(&mut self, record: SelectorExecutionRecord) {
        self.total += 1;
        self.selector_durations_ns
            .insert(record.selector.clone(), record.duration.as_nanos() as u64);
        let raw = record.raw_status.unwrap_or(record.status);
        self.raw_statuses.insert(record.selector.clone(), raw);
        match record.cache_record {
            SelectorCacheRecord::Hit => self.cache_hits += 1,
            SelectorCacheRecord::MissStored => {
                self.cache_misses += 1;
                self.cache_miss_selectors.push(record.selector.clone());
            }
            SelectorCacheRecord::MissUnstored => {
                self.cache_misses += 1;
                self.cache_unstored += 1;
                self.cache_miss_selectors.push(record.selector.clone());
                self.cache_unstored_selectors.push(record.selector.clone());
            }
        }
        match record.status {
            kiss::rpytest_runner::TestStatus::Failed => {
                self.failed += 1;
                self.failed_selectors.push(record.selector);
                self.exit_code = merge_exit_codes(self.exit_code, record.exit_code.unwrap_or(1));
            }
            kiss::rpytest_runner::TestStatus::TimedOut => {
                self.failed += 1;
                self.timed_out_selectors.push(record.selector);
                self.exit_code = merge_exit_codes(self.exit_code, record.exit_code.unwrap_or(1));
            }
            kiss::rpytest_runner::TestStatus::Passed => {
                if record.cache_record != SelectorCacheRecord::Hit {
                    self.max_passing_run_duration =
                        self.max_passing_run_duration.max(record.duration);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "execution_summary_test.rs"]
mod tests;
