//! Markdown output for layout analysis results.
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
#[path = "layout_output_test.rs"]
mod tests;

#[cfg(test)]
#[path = "layout_output_test_2.rs"]
mod layout_output_test_2;
