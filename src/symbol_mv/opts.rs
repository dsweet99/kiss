//! CLI / request options for `kiss mv`.

use std::path::PathBuf;

use crate::Language;

use super::query::ParsedQuery;

#[derive(Debug, Clone)]
pub struct MvOptions {
    pub query: String,
    pub new_name: String,
    pub paths: Vec<String>,
    pub to: Option<PathBuf>,
    pub dry_run: bool,
    pub json: bool,
    pub lang_filter: Option<Language>,
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MvRequest {
    pub query: ParsedQuery,
    pub new_name: String,
    pub paths: Vec<String>,
    pub to: Option<PathBuf>,
    pub ignore: Vec<String>,
}
