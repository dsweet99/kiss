#![cfg_attr(not(test), allow(dead_code))]
use serde::{Deserialize, Serialize};

use super::resolved::SourceRegion;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReportScope {
    pub regions: Vec<SourceRegion>,
    pub selectors: Vec<String>,
    pub complete: bool,
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
