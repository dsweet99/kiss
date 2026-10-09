mod adapt;
mod canon;
mod digest;
mod graph_store;
mod manifest;
mod projection;
mod report;
mod report_gates;
mod request_paths;
mod resolve;
mod resolved;
mod rows;
mod scope;
mod slice;
mod stamp;
mod types;

pub(crate) use adapt::{
    change_mode_from_focus, is_workspace_run, operand_raws, request_from_invocation,
    request_from_run_args, to_compat_invocation,
};
#[cfg(test)]
pub(crate) use adapt::{
    focus_from_invocation, operands_request, request_from_focus, workspace_request,
};
pub(crate) use canon::colon_to_nodeid;
pub(crate) use projection::build_slice_projection;
pub(crate) use request_paths::request_source_paths;
pub(crate) use resolve::resolve_only;
pub(crate) use rows::{AvailableRowPlan, available_rows, plan_from_available_rows};
pub(crate) use scope::ReportScope;
pub(crate) use types::{GitFocus, TargetFocus, TargetRequest};

#[cfg(test)]
#[path = "request_test.rs"]
mod request_test;

#[cfg(test)]
#[path = "stamp_test.rs"]
mod stamp_test;

#[cfg(test)]
#[path = "resolve_test.rs"]
mod resolve_test;

#[cfg(test)]
#[path = "slice_test.rs"]
mod slice_test;

#[cfg(test)]
#[path = "store_test.rs"]
mod store_test;
