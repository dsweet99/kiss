mod coverage;
mod database;
mod discovery;
mod pytest;
mod refresh;
mod skip;
mod types;
mod util;

pub const SCHEMA_VERSION: u32 = 4;
pub const RSLIP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub use coverage::{executable_lines_from_source, line_coverage};
pub use database::{load_database, write_database_atomic};
pub use discovery::{config_fingerprints, discover_repo_files, discover_tests};
pub use refresh::{
    changed_files, current_database, query_covering_tests, refresh_and_store,
    refresh_changed_tests_with_collector, refresh_with_collector,
};
pub use skip::scheduled_nodeids;
pub use types::{
    CoverageMetadata, CoveringTest, Database, FileRecord, FileRole, PytestTraceCollector,
    TestCoverageRun, TestRecord,
};
pub use util::{content_digest, db_path, normalize_path};

#[cfg(test)]
mod discovery_test;
#[cfg(test)]
mod integration_test;
#[cfg(test)]
mod refresh_test;
#[cfg(test)]
mod util_test;
