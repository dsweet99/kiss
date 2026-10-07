use std::collections::BTreeMap;

pub(crate) const RUST_IDENTITY_ENV_KEYS: &[&str] = &[
    "RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CMAKE_PREFIX_PATH",
];

const RUST_CHILD_ENV_KEYS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "TMP",
    "TEMP",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "LD_LIBRARY_PATH",
    "CC",
    "CXX",
    "CONDA_PREFIX",
    "PKG_CONFIG_PATH",
];

pub(crate) fn identity_env() -> BTreeMap<String, String> {
    let mut env = kiss::env_map_from_allowlist(RUST_IDENTITY_ENV_KEYS);
    env.extend(kiss::cargo_target_linker_env());
    if !env.contains_key("CMAKE_PREFIX_PATH")
        && let Ok(conda) = std::env::var("CONDA_PREFIX")
    {
        env.insert("CMAKE_PREFIX_PATH".to_string(), conda);
    }
    env
}

pub(crate) fn child_env() -> BTreeMap<String, String> {
    let mut env = kiss::env_map_from_allowlist(RUST_CHILD_ENV_KEYS);
    env.extend(identity_env());
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_env_holds_the_identity_env_and_drops_unlisted_variables() {
        let child = child_env();
        for (key, value) in identity_env() {
            assert_eq!(child.get(&key), Some(&value), "{key}");
        }
        assert!(!child.contains_key("LLVM_PROFILE_FILE"));
        assert!(
            child
                .keys()
                .all(|key| RUST_CHILD_ENV_KEYS.contains(&key.as_str())
                    || RUST_IDENTITY_ENV_KEYS.contains(&key.as_str())
                    || key.starts_with("CARGO_TARGET_"))
        );
    }
}
