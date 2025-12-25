//! Rust file parsing using syn

use std::path::{Path, PathBuf};

/// Error type for Rust parsing failures
#[derive(Debug)]
pub enum RustParseError {
    IoError(std::io::Error),
    SynError(syn::Error),
}

impl From<std::io::Error> for RustParseError {
    fn from(err: std::io::Error) -> Self {
        RustParseError::IoError(err)
    }
}

impl From<syn::Error> for RustParseError {
    fn from(err: syn::Error) -> Self {
        RustParseError::SynError(err)
    }
}

impl std::fmt::Display for RustParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RustParseError::IoError(e) => write!(f, "IO error: {}", e),
            RustParseError::SynError(e) => write!(f, "Syn parse error: {}", e),
        }
    }
}

impl std::error::Error for RustParseError {}

/// A parsed Rust file with its AST
pub struct ParsedRustFile {
    pub path: PathBuf,
    pub source: String,
    pub ast: syn::File,
}

/// Parses a Rust file and returns its AST
pub fn parse_rust_file(path: &Path) -> Result<ParsedRustFile, RustParseError> {
    let source = std::fs::read_to_string(path)?;
    let ast = syn::parse_file(&source)?;

    Ok(ParsedRustFile {
        path: path.to_path_buf(),
        source,
        ast,
    })
}

/// Parses all Rust files in the given paths.
/// Returns individual Results for each file.
pub fn parse_rust_files(paths: &[PathBuf]) -> Vec<Result<ParsedRustFile, RustParseError>> {
    paths.iter().map(|path| parse_rust_file(path)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn parses_simple_rust_file() {
        let mut file = NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(file, "fn main() {{ println!(\"hello\"); }}").unwrap();
        
        let parsed = parse_rust_file(file.path()).expect("should parse");
        
        assert_eq!(parsed.path, file.path());
        assert!(!parsed.source.is_empty());
        assert!(!parsed.ast.items.is_empty());
    }

    #[test]
    fn parses_rust_file_with_struct_and_impl() {
        let mut file = NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(file, r#"
struct Counter {{ value: i32 }}

impl Counter {{
    fn new() -> Self {{ Counter {{ value: 0 }} }}
    fn increment(&mut self) {{ self.value += 1; }}
}}
"#).unwrap();
        
        let parsed = parse_rust_file(file.path()).expect("should parse");
        
        assert!(parsed.ast.items.len() >= 2);
    }

    #[test]
    fn returns_error_for_invalid_rust() {
        let mut file = NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(file, "fn broken {{ }}").unwrap(); // Invalid syntax
        
        let result = parse_rust_file(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn returns_error_for_nonexistent_file() {
        let result = parse_rust_file(Path::new("nonexistent_file.rs"));
        assert!(result.is_err());
    }

    #[test]
    fn test_rust_parse_error_enum() {
        let io_err = RustParseError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "test"));
        assert!(matches!(io_err, RustParseError::IoError(_)));
    }

    #[test]
    fn test_rust_parse_error_display_fmt() {
        use std::fmt::Write;
        let err = RustParseError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "test"));
        let mut s = String::new();
        write!(&mut s, "{}", err).unwrap();
        assert!(s.contains("IO error"));
    }

    #[test]
    fn test_parsed_rust_file_struct() {
        let mut file = NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(file, "fn foo() {{}}").unwrap();
        let parsed = parse_rust_file(file.path()).unwrap();
        assert!(!parsed.source.is_empty());
        assert!(!parsed.ast.items.is_empty());
    }

    #[test]
    fn test_parse_rust_files() {
        let mut f1 = NamedTempFile::with_suffix(".rs").unwrap();
        let mut f2 = NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(f1, "fn a() {{}}").unwrap();
        writeln!(f2, "fn b() {{}}").unwrap();
        let paths = vec![f1.path().to_path_buf(), f2.path().to_path_buf()];
        let results = parse_rust_files(&paths);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }
}

