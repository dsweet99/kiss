
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub(crate) const GENERATION_SCHEMA_VERSION: &str = "rslip-python-generation-v1";
pub(crate) const POINTER_SCHEMA_VERSION: &str = "rslip-python-population-v2";
pub(crate) const RUNNER_SEMANTICS_VERSION: &str = "python-rslip-runner-v1";
pub(crate) const COLLECTOR_SEMANTICS_VERSION: &str = "python-pytest-collector-v1";

pub(crate) type CoveredLinesMap = BTreeMap<String, BTreeSet<u32>>;
pub(crate) type SelectorCoverageMap = BTreeMap<String, CoveredLinesMap>;
pub(crate) type LineIndexMap = BTreeMap<String, BTreeMap<u32, BTreeSet<String>>>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PythonExecutionIdentity {
    pub(crate) schema_version: String,
    pub(crate) runner_semantics_version: String,
    pub(crate) collector_semantics_version: String,
    pub(crate) source_root: String,
    pub(crate) interpreter_identity: String,
    pub(crate) python_version: String,
    pub(crate) pytest_version: String,
    pub(crate) plugin_identities: Vec<String>,
    pub(crate) pytest_args: Vec<String>,
    pub(crate) pytest_config_digest: String,
    pub(crate) kissconfig_test_digest: String,
    pub(crate) coverage_env_digest: String,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) input_fingerprint: String,
    pub(crate) selector_discovery_version: String,
    pub(crate) cache_schema_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PythonPopulationPlan {
    pub(crate) base_identity: PythonExecutionIdentity,
    pub(crate) selectors: Vec<String>,
}
