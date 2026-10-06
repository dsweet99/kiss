use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Lines by file that a language's stored results cover, and the lines they index as coverable.
#[derive(Debug, Default)]
pub(crate) struct StoredCoverage {
    pub(crate) covered: BTreeMap<String, BTreeSet<u32>>,
    pub(crate) coverable: BTreeMap<String, BTreeSet<u32>>,
}

/// Ids of a language's current witness and coverage generations; a report pins both.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GenerationIds {
    pub(crate) witness: Option<String>,
    pub(crate) coverage: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty() {
        let coverage = StoredCoverage::default();
        assert!(coverage.covered.is_empty() && coverage.coverable.is_empty());
        let ids = GenerationIds::default();
        assert_eq!(ids.witness, None);
        assert_eq!(ids.coverage, None);
    }
}