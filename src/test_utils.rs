use crate::parsing::{ParsedFile, create_parser, parse_file};
use std::io::Write;

pub fn parse_python_source(code: &str) -> ParsedFile {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{code}").unwrap();
    let mut parser = create_parser().unwrap();
    parse_file(&mut parser, tmp.path()).unwrap()
}
