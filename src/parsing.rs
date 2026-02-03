use rayon::prelude::*;
use std::path::{Path, PathBuf};
use tree_sitter::{Parser, Tree};

#[derive(Debug)]
pub enum ParseError {
    IoError(std::io::Error),
    ParserInitError,
    ParseFailed,
}

impl From<std::io::Error> for ParseError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "IO error: {e}"),
            Self::ParserInitError => write!(f, "Failed to initialize Python parser"),
            Self::ParseFailed => write!(f, "Failed to parse Python code"),
        }
    }
}

impl std::error::Error for ParseError {}

pub struct ParsedFile {
    pub path: PathBuf,
    pub source: String,
    pub tree: Tree,
}

pub fn create_parser() -> Result<Parser, ParseError> {
    let mut parser = Parser::new();
    let language = tree_sitter_python::LANGUAGE;
    parser
        .set_language(&language.into())
        .map_err(|_| ParseError::ParserInitError)?;
    Ok(parser)
}

pub fn parse_file(parser: &mut Parser, path: &Path) -> Result<ParsedFile, ParseError> {
    let source = std::fs::read_to_string(path)?;
    let tree = parser.parse(&source, None).ok_or(ParseError::ParseFailed)?;

    Ok(ParsedFile {
        path: path.to_path_buf(),
        source,
        tree,
    })
}

pub fn parse_files(paths: &[PathBuf]) -> Result<Vec<Result<ParsedFile, ParseError>>, ParseError> {
    enum ParserSlot {
        Ready(Parser),
        Failed,
    }

    Ok(paths
        .par_iter()
        // Creating a tree-sitter parser + setting the language is relatively expensive.
        // Reuse one parser per Rayon worker thread instead of per file.
        .map_init(
            || create_parser().map_or_else(|_| ParserSlot::Failed, ParserSlot::Ready),
            |slot, path| match slot {
                ParserSlot::Ready(parser) => parse_file(parser, path),
                ParserSlot::Failed => Err(ParseError::ParserInitError),
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_create_parser() {
        let parser = create_parser();
        assert!(parser.is_ok());
    }

    #[test]
    fn test_parse_error_display() {
        let io_err = ParseError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(io_err.to_string().contains("IO error"));
        assert_eq!(
            ParseError::ParserInitError.to_string(),
            "Failed to initialize Python parser"
        );
        assert_eq!(
            ParseError::ParseFailed.to_string(),
            "Failed to parse Python code"
        );
    }

    #[test]
    fn test_parse_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let parse_err: ParseError = io_err.into();
        matches!(parse_err, ParseError::IoError(_));
    }

    #[test]
    fn test_parse_file_success() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "def hello(): pass").unwrap();
        let mut parser = create_parser().unwrap();
        let result = parse_file(&mut parser, tmp.path());
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(parsed.source.contains("def hello"));
    }

    #[test]
    fn test_parse_file_nonexistent() {
        let mut parser = create_parser().unwrap();
        let result = parse_file(&mut parser, Path::new("/nonexistent/file.py"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_files_multiple() {
        let tmp1 = tempfile::NamedTempFile::new().unwrap();
        let tmp2 = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp1.path(), "x = 1").unwrap();
        std::fs::write(tmp2.path(), "y = 2").unwrap();
        let paths = vec![tmp1.path().to_path_buf(), tmp2.path().to_path_buf()];
        let results = parse_files(&paths).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(std::result::Result::is_ok));
    }

    #[test]
    fn test_parsed_file_struct() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "class Foo: pass").unwrap();
        let mut parser = create_parser().unwrap();
        let parsed = parse_file(&mut parser, tmp.path()).unwrap();
        assert_eq!(parsed.path, tmp.path());
        assert!(parsed.source.contains("class Foo"));
        assert!(parsed.tree.root_node().kind() == "module");
    }

    #[test]
    fn test_parse_error_display_fmt() {
        use std::fmt::Write;
        let err = ParseError::ParseFailed;
        let mut s = String::new();
        write!(&mut s, "{err}").unwrap();
        assert!(!s.is_empty());
    }
}
