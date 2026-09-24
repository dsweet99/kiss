use serde::{Deserialize, Serialize};

use super::resolved::SourceRegion;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReportScope {
    pub regions: Vec<SourceRegion>,
    pub selectors: Vec<String>,
    pub complete: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExecutionPlan {
    pub repair_selectors: Vec<String>,
    pub retry_bad: Vec<String>,
    pub forced: Vec<String>,
    pub population_repair: bool,
    pub graph_repair: bool,
}

impl ReportScope {
    pub(crate) fn from_membership(
        regions: Vec<SourceRegion>,
        mut selectors: Vec<String>,
        complete: bool,
    ) -> Self {
        selectors.sort();
        selectors.dedup();
        Self {
            regions,
            selectors,
            complete,
        }
    }
}

impl ExecutionPlan {
    pub(crate) fn known_execution_union(&self) -> Vec<String> {
        let mut union = self.repair_selectors.clone();
        union.extend(self.retry_bad.iter().cloned());
        union.extend(self.forced.iter().cloned());
        union.sort();
        union.dedup();
        union
    }
}
