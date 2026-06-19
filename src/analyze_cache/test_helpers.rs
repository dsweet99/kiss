use super::{FullCacheInputs, FullCheckCache};
use std::sync::Mutex;
use tempfile::TempDir;

pub(crate) fn empty_cache(fp: &str) -> FullCheckCache {
    FullCheckCache {
        fingerprint: fp.to_string(),
        py_stats: None,
        rs_stats: None,
        py_paths: Vec::new(),
        focus_paths: Vec::new(),
        focus_restrict: false,
        rs_paths: Vec::new(),
        py_file_count: 0,
        rs_file_count: 0,
        code_unit_count: 0,
        statement_count: 0,
        graph_nodes: 0,
        graph_edges: 0,
        base_violations: Vec::new(),
        graph_violations: Vec::new(),
        coverage_violations: Vec::new(),
        py_duplicates: Vec::new(),
        rs_duplicates: Vec::new(),
        definitions: Vec::new(),
        unreferenced: Vec::new(),
        weighted_file_pcts: Vec::new(),
        file_content_digests: Vec::new(),
        file_metadata_fingerprints: Vec::new(),
        rslip_fingerprint: String::new(),
        rust_coverage_fingerprint: String::new(),
    }
}

pub(crate) fn empty_inputs(fp: &str) -> FullCacheInputs<'static> {
    FullCacheInputs {
        fingerprint: fp.to_string(),
        py_file_count: 0,
        rs_file_count: 0,
        code_unit_count: 0,
        statement_count: 0,
        violations: &[],
        graph_viols_all: &[],
        coverage_violations: &[],
        py_graph: None,
        rs_graph: None,
        py_stats: None,
        rs_stats: None,
        focus_paths: Vec::new(),
        focus_restrict: false,
        py_paths: Vec::new(),
        rs_paths: Vec::new(),
        py_dups_all: &[],
        rs_dups_all: &[],
        definitions: Vec::new(),
        unreferenced: Vec::new(),
        weighted_file_pcts: Vec::new(),
        rslip_fingerprint: String::new(),
        rust_coverage_fingerprint: String::new(),
        include_content_digests: true,
    }
}

static HOME_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct ScopedHome {
    _guard: std::sync::MutexGuard<'static, ()>,
    pub _tmp: TempDir,
    prev: Option<std::ffi::OsString>,
}

impl ScopedHome {
    pub(crate) fn new() -> Self {
        let guard = HOME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", tmp.path()) };
        Self {
            _guard: guard,
            _tmp: tmp,
            prev,
        }
    }
}

impl Drop for ScopedHome {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}
