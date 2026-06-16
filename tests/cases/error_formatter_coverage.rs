#[test]
fn config_error_display_covers_all_variants() {
    let cases = [
        kiss::ConfigError::UnknownKey {
            key: "k".into(),
            section: "gate".into(),
        },
        kiss::ConfigError::UnknownSection {
            section: "bad".into(),
            hint: Some("gate".into()),
        },
        kiss::ConfigError::UnknownSection {
            section: "bad".into(),
            hint: None,
        },
        kiss::ConfigError::InvalidValue {
            key: "min_similarity".into(),
            message: "bad".into(),
        },
        kiss::ConfigError::ParseError {
            message: "toml".into(),
        },
        kiss::ConfigError::IoError {
            path: "x.toml".into(),
            message: "missing".into(),
        },
    ];
    for err in cases {
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn parse_error_display_covers_all_variants() {
    for err in [
        kiss::ParseError::ParserInitError,
        kiss::ParseError::ParseFailed,
        kiss::ParseError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "missing",
        )),
    ] {
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn rust_parse_error_display_covers_all_variants() {
    let syn_err = match syn::parse_file("fn (") {
        Ok(_) => panic!("expected syn parse failure"),
        Err(err) => err,
    };
    for err in [
        kiss::RustParseError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "missing",
        )),
        kiss::RustParseError::SynError(syn_err),
    ] {
        assert!(!err.to_string().is_empty());
    }
}
