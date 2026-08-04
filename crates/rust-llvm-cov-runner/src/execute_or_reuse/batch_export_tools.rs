use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::RustLlvmCovError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportTools {
    pub llvm_profdata: PathBuf,
    pub llvm_cov: PathBuf,
    pub llvm_readobj: PathBuf,
}

pub fn resolve_export_tools_from_env() -> Result<ExportTools, RustLlvmCovError> {
    let llvm_cov = env_tool_path("LLVM_COV").unwrap_or_else(find_llvm_cov_in_path);
    let llvm_profdata = env_tool_path("LLVM_PROFDATA").unwrap_or_else(find_llvm_profdata_in_path);
    let llvm_readobj = env_tool_path("LLVM_READOBJ").unwrap_or_else(find_llvm_readobj_in_path);
    Ok(ExportTools {
        llvm_cov,
        llvm_profdata,
        llvm_readobj,
    })
}

pub fn resolve_export_tools_from_rustc(rustc: &OsStr) -> Result<ExportTools, RustLlvmCovError> {
    if let Ok(tools) = resolve_export_tools_from_env()
        && tools.llvm_cov.is_file()
        && tools.llvm_profdata.is_file()
        && tools.llvm_readobj.is_file()
    {
        return Ok(tools);
    }
    let output = Command::new(rustc)
        .arg("--print")
        .arg("target-libdir")
        .output()
        .map_err(RustLlvmCovError::Io)?;
    if !output.status.success() {
        return Err(RustLlvmCovError::InvalidRequest(
            "rustc --print target-libdir failed".into(),
        ));
    }
    let libdir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let bin_dir = libdir
        .parent()
        .map(|path| path.join("bin"))
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("invalid target-libdir".into()))?;
    Ok(ExportTools {
        llvm_cov: bin_dir.join("llvm-cov"),
        llvm_profdata: bin_dir.join("llvm-profdata"),
        llvm_readobj: bin_dir.join("llvm-readobj"),
    })
}

fn env_tool_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

pub(crate) fn find_llvm_cov_in_path() -> PathBuf {
    which_tool("llvm-cov").unwrap_or_else(|| PathBuf::from("llvm-cov"))
}

pub(crate) fn find_llvm_profdata_in_path() -> PathBuf {
    which_tool("llvm-profdata").unwrap_or_else(|| PathBuf::from("llvm-profdata"))
}

pub(crate) fn find_llvm_readobj_in_path() -> PathBuf {
    which_tool("llvm-readobj").unwrap_or_else(|| PathBuf::from("llvm-readobj"))
}

pub(crate) fn parse_readobj_build_id(stdout: &[u8]) -> Option<String> {
    for line in stdout.split(|byte| *byte == b'\n') {
        let line = std::str::from_utf8(line).unwrap_or("").trim();
        let Some(id) = line.strip_prefix("Build ID:") else {
            continue;
        };
        let id = id.trim();
        if id.chars().all(|ch| ch.is_ascii_hexdigit()) && !id.is_empty() {
            return Some(id.to_ascii_lowercase());
        }
    }
    None
}

pub(crate) fn which_tool(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn parse_profdata_show_binary_ids(stdout: &[u8]) -> Vec<String> {
    let mut ids = Vec::new();
    let mut in_ids = false;
    for line in stdout.split(|byte| *byte == b'\n') {
        let line = std::str::from_utf8(line).unwrap_or("").trim();
        if line == "Binary IDs:" {
            in_ids = true;
            continue;
        }
        if !in_ids || line.is_empty() {
            continue;
        }
        if line.chars().all(|ch| ch.is_ascii_hexdigit()) {
            ids.push(line.to_ascii_lowercase());
        }
    }
    ids
}

pub(crate) fn read_profdata_binary_ids(
    tools: &ExportTools,
    profdata: &Path,
) -> Result<Vec<String>, RustLlvmCovError> {
    let output = Command::new(&tools.llvm_profdata)
        .arg("show")
        .arg("--binary-ids")
        .arg(profdata)
        .output()
        .map_err(RustLlvmCovError::Io)?;
    if !output.status.success() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "llvm-profdata show --binary-ids failed for {}",
            profdata.display()
        )));
    }
    Ok(parse_profdata_show_binary_ids(&output.stdout))
}

#[allow(dead_code)] // retained for tests and optional strict validation
pub(crate) fn objects_satisfy_profile(
    tools: &ExportTools,
    profdata: &Path,
    objects: &[PathBuf],
) -> bool {
    if objects.is_empty() {
        return false;
    }
    let mut command = Command::new(&tools.llvm_cov);
    command
        .arg("export")
        .arg("-format=text")
        .arg("--threads=1")
        .arg("-instr-profile")
        .arg(profdata)
        .arg("-check-binary-ids");
    for object in objects {
        command.arg("-object").arg(object);
    }
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod id_tests {
    use super::{
        parse_profdata_show_binary_ids, parse_readobj_build_id, resolve_export_tools_from_rustc,
    };
    use std::ffi::OsStr;

    #[test]
    fn parse_profdata_show_binary_ids_extracts_hex_values() {
        let stdout = b"Instrumentation level: Front-end\nBinary IDs: \nabc123\ndef456\n";
        assert_eq!(
            parse_profdata_show_binary_ids(stdout),
            vec!["abc123".to_string(), "def456".to_string()]
        );
    }

    #[test]
    fn resolve_export_tools_from_rustc_uses_target_triple_bin_dir() {
        let tools = resolve_export_tools_from_rustc(OsStr::new("rustc")).unwrap();
        assert!(
            tools.llvm_profdata.is_file(),
            "expected llvm-profdata at {}",
            tools.llvm_profdata.display()
        );
        assert!(
            tools.llvm_cov.is_file(),
            "expected llvm-cov at {}",
            tools.llvm_cov.display()
        );
        assert!(
            tools.llvm_readobj.is_file(),
            "expected llvm-readobj at {}",
            tools.llvm_readobj.display()
        );
    }

    #[test]
    fn parse_readobj_build_id_extracts_gnu_build_id() {
        let stdout = b"NoteSection {\n  Build ID: 09D9A43B89E5F783A58AD40DFE6710D1FD215397\n}\n";
        assert_eq!(
            parse_readobj_build_id(stdout),
            Some("09d9a43b89e5f783a58ad40dfe6710d1fd215397".to_string())
        );
    }
}
