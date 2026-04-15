//! `kiss shrink` - constrained minimization of codebase metrics.
//!
//! Workflow:
//! 1. `kiss shrink start <metric>=<target>` - save current metrics, set target
//! 2. `kiss shrink check` - run analysis and report constraint violations

mod metrics;

use serde::{Deserialize, Serialize};
use std::path::Path;

pub use metrics::{GlobalMetrics, ShrinkTarget};

const SHRINK_FILE: &str = ".kiss_shrink";

/// Persisted shrink state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShrinkState {
    /// Baseline metrics captured at `shrink start`.
    pub baseline: GlobalMetrics,
    /// The metric being minimized.
    pub target: ShrinkTarget,
    /// The target value (must be ≤ baseline value for target metric).
    pub target_value: usize,
}

impl ShrinkState {
    /// Load shrink state from `.kiss_shrink` in the current directory.
    pub fn load() -> Option<Self> {
        Self::load_from(Path::new(SHRINK_FILE))
    }

    /// Load from a specific path.
    pub fn load_from(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
    }

    /// Save to `.kiss_shrink` in the current directory.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(Path::new(SHRINK_FILE))
    }

    /// Save to a specific path.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let content =
            toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(path, content)
    }
}

/// Result of checking current metrics against shrink constraints.
#[derive(Debug, Default)]
pub struct ShrinkViolations {
    pub violations: Vec<ShrinkViolation>,
}

#[derive(Debug)]
pub struct ShrinkViolation {
    pub metric: &'static str,
    pub current: usize,
    pub limit: usize,
    pub is_target: bool,
}

impl std::fmt::Display for ShrinkViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_target {
            write!(
                f,
                "GATE_FAILED:shrink: {} {} > {} (target not met)",
                self.metric, self.current, self.limit
            )
        } else {
            write!(
                f,
                "GATE_FAILED:shrink: {} {} > {} (constraint exceeded baseline)",
                self.metric, self.current, self.limit
            )
        }
    }
}

/// Check current metrics against shrink state constraints.
pub fn check_shrink_constraints(state: &ShrinkState, current: &GlobalMetrics) -> ShrinkViolations {
    let mut violations = Vec::new();

    // Check all metrics as constraints (except target uses target_value)
    let checks: &[(ShrinkTarget, usize)] = &[
        (ShrinkTarget::Files, state.baseline.files),
        (ShrinkTarget::CodeUnits, state.baseline.code_units),
        (ShrinkTarget::Statements, state.baseline.statements),
        (ShrinkTarget::GraphNodes, state.baseline.graph_nodes),
        (ShrinkTarget::GraphEdges, state.baseline.graph_edges),
    ];

    for &(metric, baseline_limit) in checks {
        let is_target = metric == state.target;
        let limit = if is_target {
            state.target_value
        } else {
            baseline_limit
        };
        let current_val = metric.get(current);
        if current_val > limit {
            violations.push(ShrinkViolation {
                metric: metric.as_str(),
                current: current_val,
                limit,
                is_target,
            });
        }
    }

    ShrinkViolations { violations }
}

/// Parse "metric=value" argument.
pub fn parse_target_arg(arg: &str) -> Result<(ShrinkTarget, usize), String> {
    let parts: Vec<&str> = arg.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid format: '{arg}'. Expected: <metric>=<value> (e.g., graph_edges=100)"
        ));
    }
    let metric: ShrinkTarget = parts[0].parse().map_err(|()| {
        format!(
            "Unknown metric: '{}'. Valid: files, code_units, statements, graph_nodes, graph_edges",
            parts[0]
        )
    })?;
    let value: usize = parts[1]
        .parse()
        .map_err(|_| format!("Invalid value: '{}'. Expected a number.", parts[1]))?;
    Ok((metric, value))
}

#[cfg(test)]
#[path = "shrink_test.rs"]
mod tests;
