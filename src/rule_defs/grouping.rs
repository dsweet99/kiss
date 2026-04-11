use crate::config::Config;
use crate::gate_config::GateConfig;

use super::rules_table::RULES;
use super::types::RuleCategory;

pub fn rules_for_python(config: &Config, gate: &GateConfig) -> Vec<(RuleCategory, Vec<String>)> {
    rules_grouped(config, gate, true)
}

pub fn rules_for_rust(config: &Config, gate: &GateConfig) -> Vec<(RuleCategory, Vec<String>)> {
    rules_grouped(config, gate, false)
}

pub(crate) fn rules_grouped(
    config: &Config,
    gate: &GateConfig,
    python: bool,
) -> Vec<(RuleCategory, Vec<String>)> {
    use RuleCategory::{Classes, Dependencies, Duplication, Files, Functions, Testing};
    let categories = [
        Functions,
        Classes,
        Files,
        Dependencies,
        Testing,
        Duplication,
    ];
    categories
        .iter()
        .filter_map(|&cat| {
            let rules: Vec<String> = RULES
                .iter()
                .filter(|r| r.category == cat)
                .filter(|r| {
                    if python {
                        r.applies_to_python()
                    } else {
                        r.applies_to_rust()
                    }
                })
                .map(|r| r.format(config, gate))
                .collect();
            if rules.is_empty() {
                None
            } else {
                Some((cat, rules))
            }
        })
        .collect()
}
