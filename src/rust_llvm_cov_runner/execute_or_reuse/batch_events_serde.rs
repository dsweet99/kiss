use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct CargoCompilerArtifact {
    pub(super) executable: Option<String>,
    #[serde(default)]
    pub(super) filenames: Vec<String>,
    #[serde(default)]
    pub(super) manifest_path: String,
    #[serde(default)]
    pub(super) target: CargoTarget,
    #[serde(default)]
    pub(super) profile: CargoProfile,
}

#[derive(Deserialize, Default)]
pub(super) struct CargoTarget {
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) kind: Vec<String>,
    #[serde(default)]
    pub(super) src_path: String,
}

#[derive(Deserialize, Default)]
pub(super) struct CargoProfile {
    #[serde(default)]
    pub(super) test: bool,
}

#[derive(Deserialize)]
pub(super) struct CargoBuildFinished {
    pub(super) success: bool,
}

#[derive(Deserialize)]
pub(super) struct LibtestRecord {
    pub(super) event: String,
    pub(super) name: String,
    pub(super) exec_time: Option<f64>,
    pub(super) stdout: Option<String>,
    pub(super) reason: Option<String>,
}
