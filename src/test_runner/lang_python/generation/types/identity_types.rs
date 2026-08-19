
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub(crate) const GENERATION_SCHEMA_VERSION: &str = "rslip-python-generation-v1";
pub(crate) const POINTER_SCHEMA_VERSION: &str = "rslip-python-population-v2";
pub(crate) const RUNNER_SEMANTICS_VERSION: &str = "python-rslip-runner-v1";
pub(crate) const COLLECTOR_SEMANTICS_VERSION: &str = "python-pytest-collector-v1";

pub(crate) type CoveredLinesMap = BTreeMap<String, BTreeSet<u32>>;
pub(crate) type SelectorCoverageMap = BTreeMap<String, CoveredLinesMap>;
pub(crate) const LINE_INDEX_SCHEMA_V2: &str = "rslip-python-line-index-v2";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InternedLineIndex {
    #[serde(default = "line_index_schema_v2")]
    pub(crate) schema_version: String,
    #[serde(default)]
    pub(crate) selectors: Vec<String>,
    #[serde(default)]
    pub(crate) files: BTreeMap<String, BTreeMap<u32, BTreeSet<u32>>>,
    #[serde(skip)]
    pub(crate) selector_ids: BTreeMap<String, u32>,
}

pub(crate) type LineIndexMap = InternedLineIndex;

fn line_index_schema_v2() -> String {
    LINE_INDEX_SCHEMA_V2.to_string()
}

impl Default for InternedLineIndex {
    fn default() -> Self {
        Self {
            schema_version: LINE_INDEX_SCHEMA_V2.to_string(),
            selectors: Vec::new(),
            files: BTreeMap::new(),
            selector_ids: BTreeMap::new(),
        }
    }
}

impl InternedLineIndex {
    pub(crate) fn from_selectors(selectors: &[String]) -> Self {
        let mut index = Self {
            schema_version: LINE_INDEX_SCHEMA_V2.to_string(),
            selectors: selectors.to_vec(),
            files: BTreeMap::new(),
            selector_ids: BTreeMap::new(),
        };
        index.reindex();
        index
    }

    pub(crate) fn reindex(&mut self) {
        self.schema_version = LINE_INDEX_SCHEMA_V2.to_string();
        self.selector_ids = self
            .selectors
            .iter()
            .enumerate()
            .map(|(i, selector)| (selector.clone(), i as u32))
            .collect();
    }

    pub(crate) fn id_of(&self, selector: &str) -> Option<u32> {
        self.selector_ids.get(selector).copied()
    }

    pub(crate) fn name_of(&self, id: u32) -> Option<&str> {
        self.selectors.get(id as usize).map(String::as_str)
    }

    pub(crate) fn selectors_for_line(&self, file: &str, line: u32) -> Vec<&str> {
        let Some(ids) = self.files.get(file).and_then(|lines| lines.get(&line)) else {
            return Vec::new();
        };
        ids.iter().filter_map(|id| self.name_of(*id)).collect()
    }
}

pub(crate) fn decode_line_index_bytes(bytes: &[u8]) -> Result<InternedLineIndex, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if value
        .get("schema_version")
        .and_then(|v| v.as_str())
        .is_some_and(|schema| schema == LINE_INDEX_SCHEMA_V2)
    {
        let mut index: InternedLineIndex =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
        index.reindex();
        return Ok(index);
    }
    let legacy: BTreeMap<String, BTreeMap<u32, BTreeSet<String>>> =
        serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(intern_legacy_line_index(legacy))
}

fn intern_legacy_line_index(
    legacy: BTreeMap<String, BTreeMap<u32, BTreeSet<String>>>,
) -> InternedLineIndex {
    let mut names = BTreeSet::new();
    for lines in legacy.values() {
        for selectors in lines.values() {
            names.extend(selectors.iter().cloned());
        }
    }
    let selectors: Vec<String> = names.into_iter().collect();
    let mut index = InternedLineIndex::from_selectors(&selectors);
    for (file, lines) in legacy {
        for (line, names) in lines {
            let ids = names
                .iter()
                .filter_map(|name| index.id_of(name))
                .collect::<BTreeSet<_>>();
            if !ids.is_empty() {
                index.files.entry(file.clone()).or_default().insert(line, ids);
            }
        }
    }
    index
}

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
