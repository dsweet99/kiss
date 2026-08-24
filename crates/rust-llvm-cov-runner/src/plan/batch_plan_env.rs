use std::collections::BTreeMap;

pub(crate) const COVERAGE_LINK_BUILD_ID_FLAG: &str = "-Clink-arg=-Wl,--build-id=sha1";

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
    fn ensure_coverage_link_build_id_appends_flag_without_duplicating() {
        let mut env =
            BTreeMap::from([("RUSTFLAGS".to_string(), "-Cinstrument-coverage".to_string())]);
        ensure_coverage_link_build_id(&mut env);
        assert!(env["RUSTFLAGS"].contains(COVERAGE_LINK_BUILD_ID_FLAG));
        let before = env["RUSTFLAGS"].clone();
        ensure_coverage_link_build_id(&mut env);
        assert_eq!(env["RUSTFLAGS"], before);
    }
}
