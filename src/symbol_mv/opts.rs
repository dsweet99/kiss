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

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use crate::symbol_mv::query::parse_mv_query;

    impl MvOptions {
        fn witness() -> Self {
            Self {
                query: "q".into(),
                new_name: "n".into(),
                paths: vec![],
                to: None,
                dry_run: true,
                json: false,
                lang_filter: None,
                ignore: vec![],
            }
        }
    }
    impl MvRequest {
        fn witness() -> Self {
            Self {
                query: parse_mv_query("a.py::foo").unwrap(),
                new_name: "n".into(),
                paths: vec![],
                to: None,
                ignore: vec![],
            }
        }
    }

    #[test]
    fn witness_mv_opts() {
        let _ = MvOptions::witness();
        let _ = MvRequest::witness();
    }
}
