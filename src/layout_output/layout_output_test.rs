use super::*;

pub(super) fn empty_analysis(name: &str) -> LayoutAnalysis {
    LayoutAnalysis {
        project_name: name.into(),
        ..Default::default()
    }
}

#[test]
fn test_format_markdown_empty_analysis() {
    let analysis = LayoutAnalysis {
        project_name: "testproject".into(),
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("# Proposed Layout for `testproject`"));
    assert!(md.contains("No layering issues detected"));
}

#[test]
fn test_format_markdown_with_cycles() {
    let analysis = LayoutAnalysis {
        project_name: "myproject".into(),
        metrics: LayoutMetrics {
            cycle_count: 1,
            layering_violations: 0,
            cross_directory_deps: 0,
        },
        cycle_analysis: LayoutCycleAnalysis {
            cycles: vec![CycleBreakSuggestion {
                modules: vec![
                    "auth.tokens".into(),
                    "auth.session".into(),
                    "api.middleware".into(),
                ],
                suggested_break: ("auth.session".into(), "api.middleware".into()),
                reason: "middleware should depend on auth, not vice versa".into(),
            }],
        },
        layer_info: LayerInfo::default(),
        what_if: None,
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("## Cycles to Break"));
    assert!(md.contains("auth.tokens → auth.session → api.middleware"));
    assert!(md.contains("Break: `auth.session -> api.middleware`"));
    assert!(md.contains("middleware should depend on auth"));
}

#[test]
fn test_format_markdown_with_layers() {
    // 2-layer structure: Foundation (0) and Application (1)
    let analysis = LayoutAnalysis {
        project_name: "myproject".into(),
        metrics: LayoutMetrics::default(),
        cycle_analysis: LayoutCycleAnalysis::default(),
        layer_info: LayerInfo {
            layers: vec![
                vec!["utils.hash".into(), "utils.config".into()],
                vec!["models.user".into(), "models.order".into()],
            ],
        },
        what_if: None,
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("## Layers"));
    assert!(md.contains("### Layer 0: Foundation"));
    assert!(md.contains("- utils.hash"));
    assert!(md.contains("### Layer 1: Application"));
    assert!(md.contains("- models.user"));
}

#[test]
fn test_format_markdown_with_what_if() {
    let analysis = LayoutAnalysis {
        project_name: "myproject".into(),
        metrics: LayoutMetrics {
            cycle_count: 2,
            layering_violations: 0,
            cross_directory_deps: 0,
        },
        cycle_analysis: LayoutCycleAnalysis::default(),
        layer_info: LayerInfo::default(),
        what_if: Some(WhatIfAnalysis {
            remaining_cycles: 0,
            layer_count: 3,
            improvement_summary: "Breaking 2 cycles results in a clean 3-layer architecture."
                .into(),
        }),
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("## What-If Analysis"));
    assert!(md.contains("remaining_cycles: 0"));
    assert!(md.contains("layer_count: 3"));
    assert!(md.contains("Breaking 2 cycles"));
}

#[test]
fn test_format_markdown_full_example() {
    let analysis = LayoutAnalysis {
        project_name: "myproject".into(),
        metrics: LayoutMetrics {
            cycle_count: 1,
            layering_violations: 2,
            cross_directory_deps: 3,
        },
        cycle_analysis: LayoutCycleAnalysis {
            cycles: vec![CycleBreakSuggestion {
                modules: vec!["a".into(), "b".into()],
                suggested_break: ("b".into(), "a".into()),
                reason: "a is more foundational".into(),
            }],
        },
        layer_info: LayerInfo {
            layers: vec![vec!["core".into()]],
        },
        what_if: Some(WhatIfAnalysis {
            remaining_cycles: 0,
            layer_count: 2,
            improvement_summary: "Clean layering achieved.".into(),
        }),
    };
    let md = format_markdown(&analysis);

    assert!(md.contains("1 cycle"));
    assert!(md.contains("2 layering violations"));
    assert!(md.contains("3 cross-directory dependencies"));
    assert!(md.contains("### Layer 0: Foundation"));
    assert!(md.contains("- core"));
}

#[test]
fn test_layout_metrics_default() {
    let m = LayoutMetrics::default();
    assert_eq!(m.cycle_count, 0);
    assert_eq!(m.layering_violations, 0);
    assert_eq!(m.cross_directory_deps, 0);
}

#[test]
fn test_what_if_analysis_default() {
    let w = WhatIfAnalysis::default();
    assert_eq!(w.remaining_cycles, 0);
    assert_eq!(w.layer_count, 0);
    assert!(w.improvement_summary.is_empty());
}

#[test]
fn test_summary_singular_forms() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        metrics: LayoutMetrics {
            cycle_count: 1,
            layering_violations: 1,
            cross_directory_deps: 1,
        },
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("1 cycle"));
    assert!(md.contains("1 layering violation"));
    assert!(md.contains("1 cross-directory dependency"));
}

#[test]
fn test_default_layer_name_foundation() {
    assert_eq!(default_layer_name(0, 1), "Foundation");
    assert_eq!(default_layer_name(0, 3), "Foundation");
}

#[test]
fn test_default_layer_name_application() {
    assert_eq!(default_layer_name(2, 3), "Application");
    assert_eq!(default_layer_name(4, 5), "Application");
}

#[test]
fn test_default_layer_name_domain() {
    assert_eq!(default_layer_name(1, 3), "Domain");
    assert_eq!(default_layer_name(1, 5), "Domain");
}

#[test]
fn test_default_layer_name_intermediate() {
    assert_eq!(default_layer_name(2, 4), "Intermediate");
    assert_eq!(default_layer_name(3, 5), "Intermediate");
}

#[test]
fn test_three_layer_structure() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        layer_info: LayerInfo {
            layers: vec![
                vec!["foundation".into()],
                vec!["domain".into()],
                vec!["app".into()],
            ],
        },
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("Layer 0: Foundation"));
    assert!(md.contains("Layer 1: Domain"));
    assert!(md.contains("Layer 2: Application"));
}

#[test]
fn test_layout_metrics_equality() {
    let a = LayoutMetrics {
        cycle_count: 1,
        layering_violations: 2,
        cross_directory_deps: 3,
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn test_what_if_analysis_equality() {
    let a = WhatIfAnalysis {
        remaining_cycles: 0,
        layer_count: 3,
        improvement_summary: "test".into(),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn test_format_summary_cycles_only() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        metrics: LayoutMetrics {
            cycle_count: 3,
            layering_violations: 0,
            cross_directory_deps: 0,
        },
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("Current layout has 3 cycles."));
    assert!(!md.contains("layering violation"));
    assert!(!md.contains("cross-directory"));
}

#[test]
fn test_format_summary_violations_only() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        metrics: LayoutMetrics {
            cycle_count: 0,
            layering_violations: 5,
            cross_directory_deps: 0,
        },
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("Current layout has 5 layering violations."));
    assert!(!md.contains("cycle"));
    assert!(!md.contains("cross-directory"));
}

#[test]
fn test_format_summary_cross_deps_only() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        metrics: LayoutMetrics {
            cycle_count: 0,
            layering_violations: 0,
            cross_directory_deps: 2,
        },
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("Current layout has 2 cross-directory dependencies."));
    assert!(!md.contains("cycle"));
    assert!(!md.contains("layering violation"));
}

#[test]
fn test_format_cycles_empty() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        cycle_analysis: LayoutCycleAnalysis { cycles: vec![] },
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(!md.contains("## Cycles to Break"));
    assert!(!md.contains("cycles:"));
}

#[test]
fn test_format_cycles_multiple() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        metrics: LayoutMetrics {
            cycle_count: 2,
            ..Default::default()
        },
        cycle_analysis: LayoutCycleAnalysis {
            cycles: vec![
                CycleBreakSuggestion {
                    modules: vec!["a".into(), "b".into()],
                    suggested_break: ("a".into(), "b".into()),
                    reason: "reason one".into(),
                },
                CycleBreakSuggestion {
                    modules: vec!["x".into(), "y".into(), "z".into()],
                    suggested_break: ("y".into(), "z".into()),
                    reason: "reason two".into(),
                },
            ],
        },
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("# cycle 1"));
    assert!(md.contains("# cycle 2"));
    assert!(md.contains("a → b"));
    assert!(md.contains("x → y → z"));
    assert!(md.contains("1. **a ↔ b**"));
    assert!(md.contains("2. **x ↔ y ↔ z**"));
}

#[test]
fn test_format_layers_empty() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        layer_info: LayerInfo { layers: vec![] },
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(!md.contains("## Layers"));
    assert!(!md.contains("layers:"));
}

#[test]
fn test_format_layers_single_layer() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        layer_info: LayerInfo {
            layers: vec![vec!["only_module".into()]],
        },
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("## Layers"));
    assert!(md.contains("level: 0"));
    assert!(md.contains("name: \"Foundation\""));
    assert!(md.contains("- \"only_module\""));
    assert!(!md.contains("level: 1"));
}

#[test]
fn test_format_what_if_none() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        what_if: None,
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(!md.contains("## What-If Analysis"));
    assert!(!md.contains("what_if:"));
}

#[test]
fn test_format_what_if_empty_summary() {
    let analysis = LayoutAnalysis {
        project_name: "test".into(),
        what_if: Some(WhatIfAnalysis {
            remaining_cycles: 1,
            layer_count: 2,
            improvement_summary: String::new(),
        }),
        ..Default::default()
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("## What-If Analysis"));
    assert!(md.contains("remaining_cycles: 1"));
    assert!(md.contains("layer_count: 2"));
    assert!(md.contains("improvement: \"\""));
    let after_yaml = md.split("```\n\n").last().unwrap_or("");
    assert!(!after_yaml.contains("\n\n\n"));
}
