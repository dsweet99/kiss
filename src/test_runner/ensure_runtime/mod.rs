//! Shared ensure-runtime kernel and factory composition root.

mod kernel;
mod factory;
mod planning;

pub(crate) use factory::ensure_languages_runtime;
pub(crate) use kernel::ensure_runtime_cache;
pub(crate) use planning::{
    ensure_request_for_all, ensure_request_for_selectors, ensure_request_from_planned,
};

#[cfg(test)]
#[path = "kernel_test.rs"]
mod kernel_test;

#[cfg(test)]
#[path = "wiring_guard_test.rs"]
mod wiring_guard_test;

#[cfg(test)]
#[path = "planning_test.rs"]
mod planning_test;
