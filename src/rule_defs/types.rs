use crate::config::Config;
use crate::gate_config::GateConfig;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RuleCategory {
    Functions,
    Classes,
    Files,
    Dependencies,
    Testing,
    Duplication,
}

impl RuleCategory {
    pub const fn python_heading(self) -> &'static str {
        match self {
            Self::Functions => "Functions",
            Self::Classes => "Classes",
            Self::Files => "Files",
            Self::Dependencies => "Dependencies",
            Self::Testing => "Testing",
            Self::Duplication => "Duplication",
        }
    }

    pub const fn rust_heading(self) -> &'static str {
        match self {
            Self::Functions => "Functions",
            Self::Classes => "Types",
            Self::Files => "Files",
            Self::Dependencies => "Dependencies",
            Self::Testing => "Testing",
            Self::Duplication => "Duplication",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Applicability {
    Python,
    Rust,
    Both,
}

pub struct Rule {
    pub category: RuleCategory,
    pub template: &'static str,
    pub get_threshold: fn(&Config, &GateConfig) -> usize,
    pub applicability: Applicability,
}

impl Rule {
    pub fn format(&self, config: &Config, gate: &GateConfig) -> String {
        self.template
            .replace("{}", &(self.get_threshold)(config, gate).to_string())
    }

    pub const fn applies_to_python(&self) -> bool {
        matches!(
            self.applicability,
            Applicability::Python | Applicability::Both
        )
    }

    pub const fn applies_to_rust(&self) -> bool {
        matches!(
            self.applicability,
            Applicability::Rust | Applicability::Both
        )
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    #[test]
    fn witness_rule_defs_types() {
        let _ = [
            RuleCategory::Functions,
            RuleCategory::Classes,
            RuleCategory::Files,
            RuleCategory::Dependencies,
            RuleCategory::Testing,
            RuleCategory::Duplication,
        ];
        let rule = Rule {
            category: RuleCategory::Files,
            template: "{}",
            get_threshold: |_, _| 1,
            applicability: Applicability::Both,
        };
        assert!(!rule.format(&Config::default(), &GateConfig::default()).is_empty());
        assert!(rule.applies_to_python());
        assert!(rule.applies_to_rust());
        let _ = RuleCategory::Files.python_heading();
        let _ = RuleCategory::Files.rust_heading();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn threshold(_: &Config, _: &GateConfig) -> usize {
        42
    }

    #[test]
    fn rule_format_substitutes_threshold() {
        let rule = Rule {
            category: RuleCategory::Functions,
            template: "Keep functions below {} lines",
            get_threshold: threshold,
            applicability: Applicability::Both,
        };
        assert_eq!(
            rule.format(&Config::default(), &GateConfig::default()),
            "Keep functions below 42 lines"
        );
    }

    #[test]
    fn rule_category_headings_reflect_language() {
        assert_eq!(RuleCategory::Classes.python_heading(), "Classes");
        assert_eq!(RuleCategory::Classes.rust_heading(), "Types");
        assert_eq!(RuleCategory::Dependencies.python_heading(), "Dependencies");
        assert_eq!(RuleCategory::Testing.rust_heading(), "Testing");
    }

    #[test]
    fn applicability_matches_language() {
        let py = Rule {
            category: RuleCategory::Testing,
            template: "{}",
            get_threshold: threshold,
            applicability: Applicability::Python,
        };
        let rust = Rule {
            category: RuleCategory::Testing,
            template: "{}",
            get_threshold: threshold,
            applicability: Applicability::Rust,
        };
        assert!(py.applies_to_python());
        assert!(!py.applies_to_rust());
        assert!(!rust.applies_to_python());
        assert!(rust.applies_to_rust());
    }
}
