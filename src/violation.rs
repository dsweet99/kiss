//! Violation reporting types

use std::path::PathBuf;

/// A code quality violation detected during analysis
#[derive(Debug)]
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
    /// Create a new violation builder for the given file
    pub fn builder(file: impl Into<PathBuf>) -> ViolationBuilder {
        ViolationBuilder::new(file)
    }
}

/// Builder for constructing Violation instances with a fluent API
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

    pub fn line(mut self, line: usize) -> Self { self.line = line; self }
    pub fn unit_name(mut self, name: impl Into<String>) -> Self { self.unit_name = name.into(); self }
    pub fn metric(mut self, metric: impl Into<String>) -> Self { self.metric = metric.into(); self }
    pub fn value(mut self, value: usize) -> Self { self.value = value; self }
    pub fn threshold(mut self, threshold: usize) -> Self { self.threshold = threshold; self }
    pub fn message(mut self, message: impl Into<String>) -> Self { self.message = message.into(); self }
    pub fn suggestion(mut self, suggestion: impl Into<String>) -> Self { self.suggestion = suggestion.into(); self }

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
}

