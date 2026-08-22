use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum RoleBuildError {
    PythonParse {
        path: PathBuf,
        message: String,
    },
    RustParse {
        path: PathBuf,
        message: String,
    },
    CargoMetadata {
        workspace: PathBuf,
        message: String,
    },
    AmbiguousModule {
        name: String,
        rs: PathBuf,
        mod_rs: PathBuf,
    },
    MissingModule {
        from: PathBuf,
        name: String,
    },
    MissingInclude {
        from: PathBuf,
        target: PathBuf,
    },
    MalformedCfg {
        path: PathBuf,
        message: String,
    },
    CfgNestingLimit {
        path: PathBuf,
    },
}

impl fmt::Display for RoleBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PythonParse { path, message } => {
                write!(f, "Python parse failed for {}: {message}", path.display())
            }
            Self::RustParse { path, message } => {
                write!(f, "Rust parse failed for {}: {message}", path.display())
            }
            Self::CargoMetadata { workspace, message } => {
                write!(
                    f,
                    "cargo metadata failed for {}: {message}",
                    workspace.display()
                )
            }
            Self::AmbiguousModule { name, rs, mod_rs } => write!(
                f,
                "ambiguous module {name}: both {} and {} exist",
                rs.display(),
                mod_rs.display()
            ),
            Self::MissingModule { from, name } => {
                write!(f, "missing module {name} declared from {}", from.display())
            }
            Self::MissingInclude { from, target } => write!(
                f,
                "missing include {} from {}",
                target.display(),
                from.display()
            ),
            Self::MalformedCfg { path, message } => {
                write!(f, "malformed cfg in {}: {message}", path.display())
            }
            Self::CfgNestingLimit { path } => {
                write!(f, "cfg_attr nesting limit exceeded in {}", path.display())
            }
        }
    }
}

impl std::error::Error for RoleBuildError {}

#[cfg(test)]
mod error_test {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn display_covers_variants() {
        let path = PathBuf::from("a.rs");
        let msgs = [
            RoleBuildError::PythonParse {
                path: PathBuf::from("a.py"),
                message: "e".into(),
            }
            .to_string(),
            RoleBuildError::RustParse {
                path: path.clone(),
                message: "e".into(),
            }
            .to_string(),
            RoleBuildError::CargoMetadata {
                workspace: path.clone(),
                message: "e".into(),
            }
            .to_string(),
            RoleBuildError::AmbiguousModule {
                name: "m".into(),
                rs: path.clone(),
                mod_rs: path.clone(),
            }
            .to_string(),
            RoleBuildError::MissingModule {
                from: path.clone(),
                name: "m".into(),
            }
            .to_string(),
            RoleBuildError::MissingInclude {
                from: path.clone(),
                target: path.clone(),
            }
            .to_string(),
            RoleBuildError::MalformedCfg {
                path: path.clone(),
                message: "bad".into(),
            }
            .to_string(),
            RoleBuildError::CfgNestingLimit { path }.to_string(),
        ];
        let needles = [
            "Python parse",
            "Rust parse",
            "cargo metadata",
            "ambiguous module",
            "missing module",
            "missing include",
            "malformed cfg",
            "nesting limit",
        ];
        for (msg, needle) in msgs.iter().zip(needles) {
            assert!(msg.contains(needle), "{msg}");
        }
    }
}
