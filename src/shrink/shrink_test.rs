use std::str::FromStr;

use super::*;

#[test]
fn test_shrink_target_roundtrip() {
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
