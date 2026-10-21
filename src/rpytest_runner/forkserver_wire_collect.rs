use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(crate) struct WireCollect {
    pub(crate) op: &'static str,
    pub(crate) paths: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct WireCollectResult {
    pub(crate) ok: bool,
    #[serde(default)]
    pub(crate) nodeids: Vec<String>,
    #[serde(default)]
    pub(crate) observed_workspace: Vec<String>,
    #[serde(default)]
    pub(crate) unsupported_external: bool,
    #[serde(default)]
    pub(crate) python_version: String,
    #[serde(default)]
    pub(crate) pytest_version: String,
    #[serde(default)]
    pub(crate) executable: String,
    #[serde(default)]
    pub(crate) pytest_file: String,
    pub(crate) error: Option<String>,
}
