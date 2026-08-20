#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WatchCoverageResult {
    pub exit_code: i32,
    pub error: Option<String>,
    pub interrupted: bool,
}

impl WatchCoverageResult {
    pub(crate) fn ok(exit_code: i32) -> Self {
        Self {
            exit_code,
            error: None,
            interrupted: false,
        }
    }

    pub(crate) fn failed(exit_code: i32, error: impl Into<String>) -> Self {
        Self {
            exit_code,
            error: Some(error.into()),
            interrupted: false,
        }
    }

    pub(crate) fn interrupted() -> Self {
        Self {
            exit_code: 130,
            error: None,
            interrupted: true,
        }
    }
}

pub(crate) struct WatchCoverageParams<'a> {
    pub py_config: &'a kiss::Config,
    pub rs_config: &'a kiss::Config,
    pub coverage_all: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_coverage_result_ok_failed_interrupted() {
        let ok = WatchCoverageResult::ok(0);
        assert_eq!(ok.exit_code, 0);
        assert!(ok.error.is_none());
        assert!(!ok.interrupted);

        let failed = WatchCoverageResult::failed(1, "coverage gate failed");
        assert_eq!(failed.exit_code, 1);
        assert_eq!(failed.error.as_deref(), Some("coverage gate failed"));
        assert!(!failed.interrupted);

        let interrupted = WatchCoverageResult::interrupted();
        assert_eq!(interrupted.exit_code, 130);
        assert!(interrupted.error.is_none());
        assert!(interrupted.interrupted);
    }

    #[test]
    fn watch_coverage_params_hold_refs() {
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let params = WatchCoverageParams {
            py_config: &py,
            rs_config: &rs,
            coverage_all: true,
        };
        assert!(params.coverage_all);
        assert!(std::ptr::eq(params.py_config, &py));
        assert!(std::ptr::eq(params.rs_config, &rs));
    }
}
