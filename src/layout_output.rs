//! Output generation for `kiss layout` command.
//! Produces Markdown with YAML blocks for structured layout analysis.

use crate::layout_cycles::{CycleBreakSuggestion, LayoutCycleAnalysis};
use crate::layout_layers::LayerInfo;
use std::fmt::Write;

/// Summary metrics for layout analysis.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutMetrics {
    /// Number of cycles detected.
    pub cycle_count: usize,
    /// Number of layering violations (edges going from lower to higher layers).
    pub layering_violations: usize,
    /// Number of cross-directory dependencies.
    pub cross_directory_deps: usize,
}

/// What-if analysis showing potential improvement after breaking cycles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WhatIfAnalysis {
    /// Number of cycles after proposed breaks.
    pub remaining_cycles: usize,
    /// Number of layers after proposed breaks.
    pub layer_count: usize,
    /// Description of the improvement.
    pub improvement_summary: String,
}

/// Complete layout analysis results.
#[derive(Debug, Clone, Default)]
pub struct LayoutAnalysis {
    /// Name of the project being analyzed.
    pub project_name: String,
    /// Summary metrics.
    pub metrics: LayoutMetrics,
    /// Cycle analysis with break suggestions.
    pub cycle_analysis: LayoutCycleAnalysis,
    /// Computed layers (level 0 = foundation, higher = depends on lower).
    pub layer_info: LayerInfo,
    /// What-if analysis for proposed cycle breaks.
    pub what_if: Option<WhatIfAnalysis>,
}

/// Format the layout analysis as Markdown with YAML blocks.
#[must_use]
pub fn format_markdown(analysis: &LayoutAnalysis) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "# Proposed Layout for `{}`\n", analysis.project_name);

    format_summary(&mut out, analysis);
    format_cycles(&mut out, &analysis.cycle_analysis.cycles);
    format_layers(&mut out, &analysis.layer_info);
    format_what_if(&mut out, analysis.what_if.as_ref());

    out
}

fn format_summary(out: &mut String, analysis: &LayoutAnalysis) {
    out.push_str("## Summary\n\n");

    let m = &analysis.metrics;
    if m.cycle_count == 0 && m.layering_violations == 0 && m.cross_directory_deps == 0 {
        out.push_str("No layering issues detected.\n\n");
    } else {
        let mut issues = Vec::new();
        if m.cycle_count > 0 {
            issues.push(format!(
                "{} cycle{}",
                m.cycle_count,
                if m.cycle_count == 1 { "" } else { "s" }
            ));
        }
        if m.layering_violations > 0 {
            issues.push(format!(
                "{} layering violation{}",
                m.layering_violations,
                if m.layering_violations == 1 { "" } else { "s" }
            ));
        }
        if m.cross_directory_deps > 0 {
            issues.push(format!(
                "{} cross-directory dependenc{}",
                m.cross_directory_deps,
                if m.cross_directory_deps == 1 {
                    "y"
                } else {
                    "ies"
                }
            ));
        }
        let _ = writeln!(out, "Current layout has {}.\n", issues.join(" and "));
    }
}

fn format_cycles(out: &mut String, suggestions: &[CycleBreakSuggestion]) {
    if suggestions.is_empty() {
        return;
    }

    out.push_str("## Cycles to Break\n\n");
    out.push_str("```yaml\n");
    out.push_str("cycles:\n");

    for (i, suggestion) in suggestions.iter().enumerate() {
        let cycle_str = suggestion.modules.join(" → ");
        let _ = writeln!(out, "  - cycle: \"{}\"  # cycle {}", cycle_str, i + 1);
        let _ = writeln!(
            out,
            "    break: \"{} -> {}\"",
            suggestion.suggested_break.0, suggestion.suggested_break.1
        );
        let _ = writeln!(out, "    reason: \"{}\"", suggestion.reason);
    }

    out.push_str("```\n\n");

    for (i, suggestion) in suggestions.iter().enumerate() {
        let cycle_str = suggestion.modules.join(" ↔ ");
        let _ = writeln!(out, "{}. **{}**", i + 1, cycle_str);
        let _ = writeln!(
            out,
            "   - Break: `{} -> {}`",
            suggestion.suggested_break.0, suggestion.suggested_break.1
        );
        let _ = writeln!(out, "   - Reason: {}\n", suggestion.reason);
    }
}

fn format_layers(out: &mut String, layer_info: &LayerInfo) {
    if layer_info.layers.is_empty() {
        return;
    }

    out.push_str("## Layers\n\n");
    out.push_str("```yaml\n");
    out.push_str("layers:\n");

    for (level, modules) in layer_info.layers.iter().enumerate() {
        let _ = writeln!(out, "  - level: {level}");
        let name = default_layer_name(level, layer_info.num_layers());
        let _ = writeln!(out, "    name: \"{name}\"");
        out.push_str("    modules:\n");
        for module in modules {
            let _ = writeln!(out, "      - \"{module}\"");
        }
    }

    out.push_str("```\n\n");

    for (level, modules) in layer_info.layers.iter().enumerate() {
        let name = default_layer_name(level, layer_info.num_layers());
        let _ = writeln!(out, "### Layer {level}: {name}");
        for module in modules {
            let _ = writeln!(out, "- {module}");
        }
        out.push('\n');
    }
}

const fn default_layer_name(level: usize, total: usize) -> &'static str {
    // Layer naming follows a consistent pattern:
    // - Layer 0: Foundation (base utilities, no dependencies)
    // - Top layer: Application (orchestration, entry points)
    // - Layer 1 (when not top): Domain (business logic)
    // - Layers 2..top-1: Intermediate
    if level == 0 {
        "Foundation"
    } else if level == total - 1 {
        "Application"
    } else if level == 1 {
        "Domain"
    } else {
        "Intermediate"
    }
}

fn format_what_if(out: &mut String, what_if: Option<&WhatIfAnalysis>) {
    let Some(analysis) = what_if else {
        return;
    };

    out.push_str("## What-If Analysis\n\n");
    out.push_str("After breaking the suggested cycles:\n\n");
    out.push_str("```yaml\n");
    out.push_str("what_if:\n");
    let _ = writeln!(out, "  remaining_cycles: {}", analysis.remaining_cycles);
    let _ = writeln!(out, "  layer_count: {}", analysis.layer_count);
    let _ = writeln!(out, "  improvement: \"{}\"", analysis.improvement_summary);
    out.push_str("```\n\n");

    if !analysis.improvement_summary.is_empty() {
        let _ = writeln!(out, "{}", analysis.improvement_summary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_default_layer_name_two_layers() {
        // For 2 layers: Foundation (0) and Application (1)
        // The top layer is always Application
        assert_eq!(default_layer_name(0, 2), "Foundation");
        assert_eq!(default_layer_name(1, 2), "Application");

        let analysis = LayoutAnalysis {
            project_name: "test".into(),
            layer_info: LayerInfo {
                layers: vec![vec!["base".into()], vec!["top".into()]],
            },
            ..Default::default()
        };
        let md = format_markdown(&analysis);
        assert!(md.contains("Layer 0: Foundation"));
        assert!(md.contains("Layer 1: Application"));
    }

    #[test]
    fn static_coverage_touch_format_helpers() {
        fn t<T>(_: T) {}
        t(format_summary);
        t(format_cycles);
        t(format_layers);
        t(format_what_if);
    }
}
