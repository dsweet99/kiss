//! Python AST metrics for kiss.

mod body_walk;
mod compute;
mod file_stats;
mod file_walk;
mod indent_scope;
mod locals;
mod nesting;
mod parameters;
mod returns;
mod statements;
mod types;
mod walk;

pub use compute::{compute_class_metrics, compute_file_metrics, compute_function_metrics};
pub use nesting::count_node_kind;
pub use types::{ClassMetrics, FileMetrics, FunctionMetrics};
pub(crate) use walk::{PyWalkAction, walk_py_ast};

#[cfg(test)]
mod py_metrics_test;

#[cfg(test)]
mod py_metrics_test_2;
