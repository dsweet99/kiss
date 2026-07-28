use crate::duplication::CodeChunk;
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

/// Per-repo analyze/check cache root (`repo/.kiss`), matching coverage caches.
pub fn cache_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
    fn test_cache_dir_is_repo_local_kiss() {
        assert_eq!(
            cache_dir(Path::new("/repo")),
            PathBuf::from("/repo/.kiss")
        );
    }
}
