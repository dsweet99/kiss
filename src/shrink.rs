//! `kiss shrink` - constrained minimization of codebase metrics.
//!
//! Workflow:
//! 1. `kiss shrink start <metric>=<target>` - save current metrics, set target
//! 2. `kiss shrink check` - run analysis and report constraint violations

use serde::{Deserialize, Serialize};
use std::path::Path;

const SHRINK_FILE: &str = ".kiss_shrink";

/// The five top-line metrics from the "Analyzed:" summary line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalMetrics {
    pub files: usize,
    pub code_units: usize,
    pub statements: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
}

/// Which metric is being minimized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShrinkTarget {
    Files,
    CodeUnits,
    Statements,
    GraphNodes,
    GraphEdges,
}

impl std::str::FromStr for ShrinkTarget {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "files" => Ok(Self::Files),
            "code_units" => Ok(Self::CodeUnits),
            "statements" => Ok(Self::Statements),
            "graph_nodes" => Ok(Self::GraphNodes),
            "graph_edges" => Ok(Self::GraphEdges),
            _ => Err(()),
        }
    }
}

impl ShrinkTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::CodeUnits => "code_units",
            Self::Statements => "statements",
            Self::GraphNodes => "graph_nodes",
            Self::GraphEdges => "graph_edges",
        }
    }

    pub const fn get(self, m: &GlobalMetrics) -> usize {
        match self {
            Self::Files => m.files,
            Self::CodeUnits => m.code_units,
            Self::Statements => m.statements,
            Self::GraphNodes => m.graph_nodes,
            Self::GraphEdges => m.graph_edges,
        }
    }
}

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
pub fn check_shrink_constraints(
    state: &ShrinkState,
    current: &GlobalMetrics,
) -> ShrinkViolations {
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
mod tests {
    use super::*;

    #[test]
    fn test_shrink_target_roundtrip() {
        use std::str::FromStr;
        for t in [
            ShrinkTarget::Files,
            ShrinkTarget::CodeUnits,
            ShrinkTarget::Statements,
            ShrinkTarget::GraphNodes,
            ShrinkTarget::GraphEdges,
        ] {
            assert_eq!(ShrinkTarget::from_str(t.as_str()), Ok(t));
        }
        assert!(ShrinkTarget::from_str("invalid").is_err());
    }

    #[test]
    fn test_shrink_target_get() {
        let m = GlobalMetrics {
            files: 10,
            code_units: 20,
            statements: 30,
            graph_nodes: 40,
            graph_edges: 50,
        };
        assert_eq!(ShrinkTarget::Files.get(&m), 10);
        assert_eq!(ShrinkTarget::CodeUnits.get(&m), 20);
        assert_eq!(ShrinkTarget::Statements.get(&m), 30);
        assert_eq!(ShrinkTarget::GraphNodes.get(&m), 40);
        assert_eq!(ShrinkTarget::GraphEdges.get(&m), 50);
    }

    #[test]
    fn test_parse_target_arg() {
        let (t, v) = parse_target_arg("graph_edges=100").unwrap();
        assert_eq!(t, ShrinkTarget::GraphEdges);
        assert_eq!(v, 100);

        let (t, v) = parse_target_arg("statements=500").unwrap();
        assert_eq!(t, ShrinkTarget::Statements);
        assert_eq!(v, 500);

        assert!(parse_target_arg("invalid=100").is_err());
        assert!(parse_target_arg("files=abc").is_err());
        assert!(parse_target_arg("no_equals").is_err());
    }

    #[test]
    fn test_shrink_state_save_load() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let state = ShrinkState {
            baseline: GlobalMetrics {
                files: 44,
                code_units: 950,
                statements: 3223,
                graph_nodes: 56,
                graph_edges: 126,
            },
            target: ShrinkTarget::GraphEdges,
            target_value: 100,
        };
        state.save_to(tmp.path()).unwrap();
        let loaded = ShrinkState::load_from(tmp.path()).unwrap();
        assert_eq!(loaded.target, ShrinkTarget::GraphEdges);
        assert_eq!(loaded.target_value, 100);
        assert_eq!(loaded.baseline.files, 44);
    }

    #[test]
    fn test_check_shrink_constraints_no_violations() {
        let state = ShrinkState {
            baseline: GlobalMetrics {
                files: 44,
                code_units: 950,
                statements: 3223,
                graph_nodes: 56,
                graph_edges: 126,
            },
            target: ShrinkTarget::GraphEdges,
            target_value: 100,
        };
        let current = GlobalMetrics {
            files: 44,
            code_units: 948,
            statements: 3200,
            graph_nodes: 56,
            graph_edges: 95,
        };
        let result = check_shrink_constraints(&state, &current);
        assert!(result.violations.is_empty());
    }

    #[test]
    fn test_check_shrink_constraints_with_violations() {
        let state = ShrinkState {
            baseline: GlobalMetrics {
                files: 44,
                code_units: 950,
                statements: 3223,
                graph_nodes: 56,
                graph_edges: 126,
            },
            target: ShrinkTarget::GraphEdges,
            target_value: 100,
        };
        // Target violated, constraint (code_units) also violated
        let current = GlobalMetrics {
            files: 44,
            code_units: 960, // > 950 baseline
            statements: 3200,
            graph_nodes: 56,
            graph_edges: 110, // > 100 target
        };
        let result = check_shrink_constraints(&state, &current);
        assert_eq!(result.violations.len(), 2);

        let target_viol = result.violations.iter().find(|v| v.is_target).unwrap();
        assert_eq!(target_viol.metric, "graph_edges");
        assert_eq!(target_viol.current, 110);
        assert_eq!(target_viol.limit, 100);

        let constraint_viol = result.violations.iter().find(|v| !v.is_target).unwrap();
        assert_eq!(constraint_viol.metric, "code_units");
        assert_eq!(constraint_viol.current, 960);
        assert_eq!(constraint_viol.limit, 950);
    }

    #[test]
    fn test_shrink_violation_display() {
        let target_viol = ShrinkViolation {
            metric: "graph_edges",
            current: 110,
            limit: 100,
            is_target: true,
        };
        assert_eq!(
            target_viol.to_string(),
            "GATE_FAILED:shrink: graph_edges 110 > 100 (target not met)"
        );

        let constraint_viol = ShrinkViolation {
            metric: "code_units",
            current: 960,
            limit: 950,
            is_target: false,
        };
        assert_eq!(
            constraint_viol.to_string(),
            "GATE_FAILED:shrink: code_units 960 > 950 (constraint exceeded baseline)"
        );
    }
}
