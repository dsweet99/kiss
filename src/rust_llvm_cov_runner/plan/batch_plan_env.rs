use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub(crate) const COVERAGE_LINK_BUILD_ID_FLAG: &str = "-Clink-arg=-Wl,--build-id=sha1";

pub(crate) fn normalize_path_env(env: &mut BTreeMap<String, String>) {
    let Some(path) = env.get_mut("PATH") else {
        return;
    };
    let separator = if cfg!(windows) { ';' } else { ':' };
    let mut seen = BTreeSet::new();
    *path = path
        .split(separator)
        .filter(|entry| seen.insert((*entry).to_string()))
        .collect::<Vec<_>>()
        .join(&separator.to_string());
}

pub(crate) fn normalized_request_environment(
    input: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env = input.clone();
    normalize_path_env(&mut env);
    env
}

const IDENTITY_PATH_TOOLS: &[&str] = &[
    "cargo",
    "rustc",
    "cmake",
    "cc",
    "c++",
    "pkg-config",
    "make",
    "cargo-llvm-cov",
    "cargo-nextest",
];

pub(crate) fn resolved_identity_tools(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> BTreeMap<String, String> {
    let Some(path) = env.get("PATH") else {
        return BTreeMap::new();
    };
    IDENTITY_PATH_TOOLS
        .iter()
        .filter_map(|name| {
            resolve_executable_on_path(path, cwd, env, name)
                .map(|resolved| ((*name).to_string(), resolved))
        })
        .collect()
}

fn resolve_executable_on_path(
    path: &str,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    name: &str,
) -> Option<String> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    path.split(separator).find_map(|dir| {
        let directory = Path::new(dir);
        let directory = if directory.is_absolute() {
            directory.to_path_buf()
        } else {
            cwd.join(directory)
        };
        executable_names(env, name).into_iter().find_map(|name| {
            let candidate = directory.join(name);
            let meta = fs::metadata(&candidate).ok()?;
            if !meta.is_file() || !is_executable(&meta) {
                return None;
            }
            Some(candidate.to_string_lossy().into_owned())
        })
    })
}

fn executable_names(env: &BTreeMap<String, String>, name: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        if Path::new(name).extension().is_some() {
            return vec![name.to_string()];
        }
        let extensions = env
            .get("PATHEXT")
            .cloned()
            .or_else(|| std::env::var("PATHEXT").ok())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
        extensions
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| format!("{name}{extension}"))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = env;
        vec![name.to_string()]
    }
}

fn is_executable(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

pub(crate) fn ensure_coverage_link_build_id(env: &mut BTreeMap<String, String>) {
    if env
        .get("RUSTFLAGS")
        .is_some_and(|flags| flags.contains("build-id"))
    {
        return;
    }
    match env.get_mut("RUSTFLAGS") {
        Some(flags) => {
            if !flags.is_empty() {
                flags.push(' ');
            }
            flags.push_str(COVERAGE_LINK_BUILD_ID_FLAG);
        }
        None => {
            env.insert(
                "RUSTFLAGS".to_string(),
                COVERAGE_LINK_BUILD_ID_FLAG.to_string(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_identity_tools_respects_empty_path_entry_as_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let cmake = tmp.path().join("cmake");
        fs::write(&cmake, b"stub\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cmake, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let separator = if cfg!(windows) { ';' } else { ':' };
        let env = BTreeMap::from([("PATH".to_string(), format!("{separator}/missing"))]);

        let tools = resolved_identity_tools(&env, tmp.path());

        assert_eq!(
            tools.get("cmake").map(String::as_str),
            Some(cmake.to_str().unwrap())
        );
    }

    #[cfg(windows)]
    #[test]
    fn resolved_identity_tools_uses_pathext() {
        let tmp = tempfile::tempdir().unwrap();
        let cmake = tmp.path().join("cmake.exe");
        fs::write(&cmake, b"stub\n").unwrap();
        let env = BTreeMap::from([
            (
                "PATH".to_string(),
                tmp.path().to_string_lossy().into_owned(),
            ),
            ("PATHEXT".to_string(), ".EXE".to_string()),
        ]);

        let tools = resolved_identity_tools(&env, tmp.path());

        assert_eq!(
            tools.get("cmake").map(String::as_str),
            Some(cmake.to_str().unwrap())
        );
    }

    #[test]
    fn ensure_coverage_link_build_id_appends_flag_without_duplicating() {
        let mut env =
            BTreeMap::from([("RUSTFLAGS".to_string(), "-Cinstrument-coverage".to_string())]);
        ensure_coverage_link_build_id(&mut env);
        assert!(env["RUSTFLAGS"].contains(COVERAGE_LINK_BUILD_ID_FLAG));
        let before = env["RUSTFLAGS"].clone();
        ensure_coverage_link_build_id(&mut env);
        assert_eq!(env["RUSTFLAGS"], before);
    }

    #[test]
    fn normalize_path_env_removes_only_later_exact_duplicates() {
        let separator = if cfg!(windows) { ';' } else { ':' };
        let mut env = BTreeMap::from([(
            "PATH".to_string(),
            ["/first", "/second", "/first", ""].join(&separator.to_string()),
        )]);

        normalize_path_env(&mut env);

        assert_eq!(
            env["PATH"],
            ["/first", "/second", ""].join(&separator.to_string())
        );
    }
}
