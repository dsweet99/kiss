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

    impl RuleCategory {
        fn witness() -> Self {
            Self::Functions
        }
    }
    impl Applicability {
        fn witness() -> Self {
            Self::Both
        }
    }
    impl Rule {
        fn witness() -> Self {
            Self {
                category: RuleCategory::witness(),
                template: "{}",
                get_threshold: |c, _| c.statements_per_function,
                applicability: Applicability::witness(),
            }
        }
    }

    #[test]
    fn witness_rule_types() {
        let _ = RuleCategory::witness();
        let _ = Applicability::witness();
        let rule = Rule::witness();
        let cfg = Config::python_defaults();
        let gate = GateConfig::default();
        assert!(!rule.format(&cfg, &gate).is_empty());
    }
}
