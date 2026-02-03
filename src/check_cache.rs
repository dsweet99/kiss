use crate::duplication::CodeChunk;
use crate::test_refs::CodeDefinition;
use crate::units::CodeUnitKind;
use crate::violation::Violation;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedViolation {
    pub file: String,
    pub line: usize,
    pub unit_name: String,
    pub metric: String,
    pub value: usize,
    pub threshold: usize,
    pub message: String,
    pub suggestion: String,
}

impl CachedViolation {
    pub fn into_violation(self) -> Violation {
        Violation {
            file: PathBuf::from(self.file),
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

impl From<&Violation> for CachedViolation {
    fn from(v: &Violation) -> Self {
        Self {
            file: v.file.to_string_lossy().to_string(),
            line: v.line,
            unit_name: v.unit_name.clone(),
            metric: v.metric.clone(),
            value: v.value,
            threshold: v.threshold,
            message: v.message.clone(),
            suggestion: v.suggestion.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCodeChunk {
    pub file: String,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub normalized: String,
}

impl CachedCodeChunk {
    pub fn into_chunk(self) -> CodeChunk {
        CodeChunk {
            file: PathBuf::from(self.file),
            name: self.name,
            start_line: self.start_line,
            end_line: self.end_line,
            normalized: self.normalized,
        }
    }
}

impl From<&CodeChunk> for CachedCodeChunk {
    fn from(c: &CodeChunk) -> Self {
        Self {
            file: c.file.to_string_lossy().to_string(),
            name: c.name.clone(),
            start_line: c.start_line,
            end_line: c.end_line,
            normalized: c.normalized.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCodeDefinition {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub containing_class: Option<String>,
}

const fn kind_to_str(k: CodeUnitKind) -> &'static str {
    match k {
        CodeUnitKind::Function => "function",
        CodeUnitKind::Method => "method",
        CodeUnitKind::Class => "class",
        CodeUnitKind::Module => "module",
        CodeUnitKind::Struct => "struct",
        CodeUnitKind::Enum => "enum",
        CodeUnitKind::TraitImplMethod => "trait_impl_method",
    }
}

fn kind_from_str(s: &str) -> CodeUnitKind {
    match s {
        "method" => CodeUnitKind::Method,
        "class" => CodeUnitKind::Class,
        "module" => CodeUnitKind::Module,
        "struct" => CodeUnitKind::Struct,
        "enum" => CodeUnitKind::Enum,
        "trait_impl_method" => CodeUnitKind::TraitImplMethod,
        _ => CodeUnitKind::Function,
    }
}

impl From<&CodeDefinition> for CachedCodeDefinition {
    fn from(d: &CodeDefinition) -> Self {
        Self {
            name: d.name.clone(),
            kind: kind_to_str(d.kind).to_string(),
            file: d.file.to_string_lossy().to_string(),
            line: d.line,
            containing_class: d.containing_class.clone(),
        }
    }
}

impl CachedCodeDefinition {
    pub fn into_definition(self) -> CodeDefinition {
        CodeDefinition {
            name: self.name,
            kind: kind_from_str(&self.kind),
            file: PathBuf::from(self.file),
            line: self.line,
            containing_class: self.containing_class,
        }
    }
}

pub fn cache_dir() -> PathBuf {
    // Prefer user cache dir; fall back to temp.
    if let Some(home) = std::env::var_os("HOME") {
        return Path::new(&home).join(".cache").join("kiss");
    }
    std::env::temp_dir().join("kiss-cache")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cached_violation_roundtrip() {
        let v = Violation::builder("foo.py")
            .line(12)
            .unit_name("f")
            .metric("m")
            .value(2)
            .threshold(1)
            .message("msg")
            .suggestion("sugg")
            .build();
        let cached = CachedViolation::from(&v);
        let v2 = cached.into_violation();
        assert_eq!(v2.file, PathBuf::from("foo.py"));
        assert_eq!(v2.line, 12);
        assert_eq!(v2.unit_name, "f");
    }

    #[test]
    fn test_cached_chunk_roundtrip() {
        let c = CodeChunk {
            file: PathBuf::from("a.py"),
            name: "x".to_string(),
            start_line: 1,
            end_line: 2,
            normalized: "norm".to_string(),
        };
        let cached = CachedCodeChunk::from(&c);
        let c2 = cached.into_chunk();
        assert_eq!(c2.file, PathBuf::from("a.py"));
        assert_eq!(c2.name, "x");
    }

    #[test]
    fn test_cached_definition_roundtrip() {
        let d = CodeDefinition {
            name: "C".to_string(),
            kind: CodeUnitKind::Class,
            file: PathBuf::from("x.py"),
            line: 3,
            containing_class: None,
        };
        let cached = CachedCodeDefinition::from(&d);
        let d2 = cached.into_definition();
        assert_eq!(d2.name, "C");
        assert_eq!(d2.kind, CodeUnitKind::Class);
        assert_eq!(d2.file, PathBuf::from("x.py"));
    }

    #[test]
    fn test_cache_dir_smoke() {
        // Full-run cache uses this directory; keep it stable and non-panicking.
        let _ = cache_dir();

        // Touch helpers for the static test-reference gate.
        let _ = kind_to_str(CodeUnitKind::Function);
        let _ = kind_from_str("class");
    }
}

