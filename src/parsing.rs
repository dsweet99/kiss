//! Python file parsing using tree-sitter

use std::path::{Path, PathBuf};
use tree_sitter::{Parser, Tree};

/// Error type for parsing failures
#[derive(Debug)]
pub enum ParseError {
    IoError(std::io::Error),
    ParserInitError,
    ParseFailed,
}

impl From<std::io::Error> for ParseError {
    fn from(err: std::io::Error) -> Self {
        ParseError::IoError(err)
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::IoError(e) => write!(f, "IO error: {}", e),
            ParseError::ParserInitError => write!(f, "Failed to initialize Python parser"),
            ParseError::ParseFailed => write!(f, "Failed to parse Python code"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A parsed Python file with its AST
pub struct ParsedFile {
    pub path: PathBuf,
    pub source: String,
    pub tree: Tree,
}

/// Creates a tree-sitter parser configured for Python
pub fn create_parser() -> Result<Parser, ParseError> {
    let mut parser = Parser::new();
    let language = tree_sitter_python::LANGUAGE;
    parser
        .set_language(&language.into())
        .map_err(|_| ParseError::ParserInitError)?;
    Ok(parser)
}

/// Parses a Python file and returns its AST
pub fn parse_file(parser: &mut Parser, path: &Path) -> Result<ParsedFile, ParseError> {
    let source = std::fs::read_to_string(path)?;
    let tree = parser.parse(&source, None).ok_or(ParseError::ParseFailed)?;

    Ok(ParsedFile {
        path: path.to_path_buf(),
        source,
        tree,
    })
}

/// Parses all Python files in the given paths.
/// Returns Err if parser initialization fails; individual file errors are in the inner Results.
pub fn parse_files(
    paths: &[PathBuf],
) -> Result<Vec<Result<ParsedFile, ParseError>>, ParseError> {
    let mut parser = create_parser()?;

    Ok(paths
        .iter()
        .map(|path| parse_file(&mut parser, path))
        .collect())
}

