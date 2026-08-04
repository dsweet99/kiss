use std::collections::{BTreeMap, BTreeSet};

use crate::{RustLineCoverage, RustTestBinaryIdentity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoverageOutputMode {
    SelectorEntries,
    CheckAggregate {
        publication_binary_ids: Option<BTreeSet<String>>,
        repair_publication: Option<CheckAggregateRepairPublication>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckAggregateRepairPublication {
    /// Generation fingerprint of the reusable prior check-aggregate whose
    /// selector entries supply retained timings/coverage bindings.
    pub prior_generation: String,
    pub selector_binary_ids: BTreeMap<String, Vec<String>>,
    pub test_binaries: Vec<RustTestBinaryIdentity>,
    pub retained_binary_line_maps: BTreeMap<String, RustLineCoverage>,
}
