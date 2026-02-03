use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub unit_name: String,
    pub metric: String,
    pub value: usize,
    pub threshold: usize,
    pub message: String,
    pub suggestion: String,
}

impl Violation {
    pub fn builder(file: impl Into<PathBuf>) -> ViolationBuilder {
        ViolationBuilder::new(file)
    }
}

pub struct ViolationBuilder {
    file: PathBuf,
    line: usize,
    unit_name: String,
    metric: String,
    value: usize,
    threshold: usize,
    message: String,
    suggestion: String,
}

impl ViolationBuilder {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            line: 1,
            unit_name: String::new(),
            metric: String::new(),
            value: 0,
            threshold: 0,
            message: String::new(),
            suggestion: String::new(),
        }
    }

    #[must_use]
    pub const fn line(mut self, line: usize) -> Self {
        self.line = line;
        self
    }
    #[must_use]
    pub fn unit_name(mut self, name: impl Into<String>) -> Self {
        self.unit_name = name.into();
        self
    }
    #[must_use]
    pub fn metric(mut self, metric: impl Into<String>) -> Self {
        self.metric = metric.into();
        self
    }
    #[must_use]
    pub const fn value(mut self, value: usize) -> Self {
        self.value = value;
        self
    }
    #[must_use]
    pub const fn threshold(mut self, threshold: usize) -> Self {
        self.threshold = threshold;
        self
    }
    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }
    #[must_use]
    pub fn suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = suggestion.into();
        self
    }

    pub fn build(self) -> Violation {
        Violation {
            file: self.file,
            line: self.line,
            unit_name: self.unit_name,
            metric: self.metric,
            value: self.value,
            threshold: self.threshold,
            message: self.message,
            suggestion: self.suggestion,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_violation_builder() {
        let v = Violation::builder("test.py")
            .line(10)
            .unit_name("foo")
            .metric("statements")
            .value(50)
            .threshold(30)
            .message("Too many statements")
            .suggestion("Break it up")
            .build();

        assert_eq!(v.file.to_str().unwrap(), "test.py");
        assert_eq!(v.line, 10);
        assert_eq!(v.unit_name, "foo");
        assert_eq!(v.metric, "statements");
        assert_eq!(v.value, 50);
        assert_eq!(v.threshold, 30);
        assert_eq!(v.message, "Too many statements");
        assert_eq!(v.suggestion, "Break it up");
    }

    #[test]
    fn test_violation_builder_default_line() {
        let v = ViolationBuilder::new("test.py").build();
        assert_eq!(v.line, 1);
    }

    #[test]
    fn test_violation_has_all_required_fields() {
        let v = Violation::builder("src/foo.py")
            .line(42)
            .unit_name("process_data")
            .metric("statements_per_function")
            .value(75)
            .threshold(50)
            .message("Function has 75 statements (threshold: 50)")
            .suggestion("Break into smaller, focused functions.")
            .build();

        assert!(!v.file.to_string_lossy().is_empty(), "file must be set");
        assert!(v.line > 0, "line must be positive");
        assert!(!v.unit_name.is_empty(), "unit_name must be set");
        assert!(!v.metric.is_empty(), "metric must be set");
        assert!(
            v.value > v.threshold,
            "violation should have value > threshold"
        );
        assert!(!v.message.is_empty(), "message must be set");
        assert!(!v.suggestion.is_empty(), "suggestion must be set");
    }

    #[test]
    fn test_violation_suggestion_is_actionable() {
        let v = Violation::builder("test.py")
            .suggestion("Break into smaller, focused functions.")
            .build();

        let suggestion = v.suggestion.to_lowercase();
        let has_action_word = suggestion.contains("break")
            || suggestion.contains("extract")
            || suggestion.contains("split")
            || suggestion.contains("reduce")
            || suggestion.contains("move")
            || suggestion.contains("use")
            || suggestion.contains("consider")
            || suggestion.contains("introduce");

        assert!(has_action_word, "suggestion should contain actionable verb");
    }
}
