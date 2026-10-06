use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Lines by file that a language's stored results cover.
#[derive(Debug, Default)]
pub(crate) struct StoredCoverage {
    pub(crate) covered: BTreeMap<String, BTreeSet<u32>>,
}

/// Id of a language's current witness generation; a report pins it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GenerationIds {
    pub(crate) witness: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty() {
        let coverage = StoredCoverage::default();
        assert!(coverage.covered.is_empty());
        let ids = GenerationIds::default();
        assert_eq!(ids.witness, None);
    }
}
