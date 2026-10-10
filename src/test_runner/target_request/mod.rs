mod adapt;
mod canon;
mod digest;
mod manifest;
mod projection;
mod request_paths;
mod resolve;
mod resolved;
mod slice;
mod stamp;
mod types;

#[cfg(test)]
pub(crate) use adapt::{
    focus_from_invocation, operands_request, request_from_focus, workspace_request,
};
pub(crate) use adapt::{
    is_workspace_run, operand_raws, request_from_invocation, request_from_run_args,
    to_compat_invocation,
};
pub(crate) use canon::colon_to_nodeid;
#[cfg(test)]
pub(crate) use projection::build_slice_projection;
pub(crate) use request_paths::request_source_paths;
pub(crate) use resolve::git_resolve_args;
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
