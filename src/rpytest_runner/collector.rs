use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

const COLLECT_JSON_PREFIX: &str = "KISS_COLLECT_JSON:";

const PYTEST_COLLECT_MAIN: &str = r#"
import json
import os
import sys

os.environ.pop("PYTEST_ADDOPTS", None)
os.environ["PYTEST_DISABLE_PLUGIN_AUTOLOAD"] = "1"

_obs=set(); _ext=False
_root=os.path.realpath(os.getcwd())
def _prefixes():
    out={sys.prefix, getattr(sys,'base_prefix',sys.prefix), getattr(sys,'exec_prefix',sys.prefix)}
    try:
        import site
        out.update(site.getsitepackages())
        out.add(site.getusersitepackages())
    except Exception:
        pass
    return tuple(os.path.realpath(p) for p in out if p)
_pref=_prefixes()
def _hook(event, args):
    global _ext
    if event!='open' or not isinstance(args[0], str):
        return
    try:
        real=os.path.realpath(args[0])
    except Exception:
        return
    if real.startswith(_root+os.sep) or real==_root:
        if os.path.isfile(real):
            _obs.add(os.path.relpath(real, _root).replace('\\\\','/'))
        return
    if any(real==p or real.startswith(p+os.sep) for p in _pref):
        return
    if real.startswith(('/dev','/proc','/sys','/tmp','/var/tmp')):
        return
    _ext=True
sys.addaudithook(_hook)

class _KissCollectReporter:
    def pytest_collection_finish(self, session):
        payload = {
            "nodeids": [item.nodeid for item in session.items],
            "observed_workspace": sorted(_obs),
            "unsupported_external": _ext,
        }
        sys.stdout.write("KISS_COLLECT_JSON:" + json.dumps(payload) + "\n")

def main():
    config = json.loads(sys.stdin.read())
    # Clear addopts (avoids inherited random-order/full-trace/etc), but keep
    # importlib mode so explicit multi-path collection does not collide on
    # shared basenames such as conftest.py under pytest's default prepend mode.
    args = [
        "--collect-only",
        "-q",
        "-o",
        "addopts=",
        "--import-mode=importlib",
    ]
    args.extend(config.get("pytest_args", []))
    args.extend(config.get("paths", []))
    raise SystemExit(pytest.main(args, plugins=[_KissCollectReporter()]))

import pytest

main()
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PytestCollectRequest {
    pub cwd: PathBuf,
    pub python: PathBuf,
    pub paths: Vec<PathBuf>,
    pub pytest_args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PytestCollectOutcome {
    pub nodeids: Vec<String>,
    pub observed_workspace: Vec<String>,
    pub unsupported_external: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PytestCollectError {
    InvalidRequest(String),
    Spawn {
        program: PathBuf,
        message: String,
    },
    CollectionFailed {
        exit_code: Option<i32>,
        stderr: String,
        stdout: String,
    },
    InvalidOutput(String),
    NodeidNormalization {
        nodeid: String,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SubprocessPytestCollector;

impl SubprocessPytestCollector {
    pub fn new() -> Self {
        Self
    }

    pub fn collect(
        &self,
        req: PytestCollectRequest,
    ) -> Result<PytestCollectOutcome, PytestCollectError> {
        collect_subprocess(req)
    }
}

pub fn subprocess_pytest_collector() -> SubprocessPytestCollector {
    SubprocessPytestCollector::new()
}

pub fn collect_pytest_nodeids(
    req: PytestCollectRequest,
) -> Result<PytestCollectOutcome, PytestCollectError> {
    SubprocessPytestCollector::new().collect(req)
}

fn collect_subprocess(
    req: PytestCollectRequest,
) -> Result<PytestCollectOutcome, PytestCollectError> {
    validate_collect_request(&req)?;
    let config = collect_config_json(&req.paths, &req.pytest_args)?;
    let mut cmd = Command::new(&req.python);
    cmd.current_dir(&req.cwd);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.env_remove("PYTEST_ADDOPTS");
    cmd.env("PYTEST_DISABLE_PLUGIN_AUTOLOAD", "1");
    for (key, value) in &req.env {
        cmd.env(key, value);
    }
    cmd.arg("-c").arg(PYTEST_COLLECT_MAIN);

    let mut child = cmd.spawn().map_err(|err| PytestCollectError::Spawn {
        program: req.python.clone(),
        message: err.to_string(),
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(config.as_bytes())
            .map_err(|err| PytestCollectError::Spawn {
                program: req.python.clone(),
                message: err.to_string(),
            })?;
    }
    let output = child
        .wait_with_output()
        .map_err(|err| PytestCollectError::Spawn {
            program: req.python.clone(),
            message: err.to_string(),
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let parsed = parse_collect_payload(&stdout)?;
    if !output.status.success()
        && parsed.nodeids.is_empty()
        && !is_empty_collection_success(output.status.code(), &parsed)
    {
        return Err(PytestCollectError::CollectionFailed {
            exit_code: output.status.code(),
            stderr,
            stdout,
        });
    }
    let nodeids = normalize_nodeids(&parsed.nodeids, &req.cwd)?;
    Ok(PytestCollectOutcome {
        nodeids,
        observed_workspace: parsed.observed_workspace,
        unsupported_external: parsed.unsupported_external,
    })
}

fn is_empty_collection_success(exit_code: Option<i32>, parsed: &CollectPayload) -> bool {
    exit_code == Some(5) && parsed.nodeids.is_empty()
}

fn validate_collect_request(req: &PytestCollectRequest) -> Result<(), PytestCollectError> {
    if req.cwd.as_os_str().is_empty() {
        return Err(PytestCollectError::InvalidRequest(
            "pytest collect cwd must not be empty".to_string(),
        ));
    }
    if req.python.as_os_str().is_empty() {
        return Err(PytestCollectError::InvalidRequest(
            "python executable must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn collect_config_json(
    paths: &[PathBuf],
    pytest_args: &[String],
) -> Result<String, PytestCollectError> {
    let paths = paths
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    serde_json::to_string(&CollectConfigPayload {
        paths,
        pytest_args: pytest_args.to_vec(),
    })
    .map_err(|err| PytestCollectError::InvalidRequest(err.to_string()))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CollectConfigPayload {
    paths: Vec<String>,
    pytest_args: Vec<String>,
}

impl CollectConfigPayload {
    #[cfg(test)]
    fn witness() -> Self {
        Self {
            paths: vec!["tests/t.py".to_string()],
            pytest_args: vec!["-q".to_string()],
        }
    }
}

#[derive(Deserialize)]
struct CollectPayload {
    nodeids: Vec<String>,
    #[serde(default)]
    observed_workspace: Vec<String>,
    #[serde(default)]
    unsupported_external: bool,
}

impl CollectPayload {
    #[cfg(test)]
    fn witness() -> Self {
        Self {
            nodeids: vec!["tests/a.py::t".to_string()],
            observed_workspace: Vec::new(),
            unsupported_external: false,
        }
    }
}

fn parse_collect_payload(stdout: &str) -> Result<CollectPayload, PytestCollectError> {
    let line = stdout
        .lines()
        .find(|line| line.starts_with(COLLECT_JSON_PREFIX))
        .ok_or_else(|| {
            PytestCollectError::InvalidOutput(
                "pytest collection output did not include a JSON payload".to_string(),
            )
        })?;
    let json = line.strip_prefix(COLLECT_JSON_PREFIX).ok_or_else(|| {
        PytestCollectError::InvalidOutput("invalid collection prefix".to_string())
    })?;
    serde_json::from_str(json).map_err(|err| PytestCollectError::InvalidOutput(err.to_string()))
}

pub(crate) fn normalize_nodeids(
    nodeids: &[String],
    repo_root: &Path,
) -> Result<Vec<String>, PytestCollectError> {
    nodeids
        .iter()
        .map(|nodeid| normalize_nodeid(nodeid, repo_root))
        .collect()
}

pub(crate) fn normalize_nodeid(
    nodeid: &str,
    repo_root: &Path,
) -> Result<String, PytestCollectError> {
    let Some((file_part, rest)) = nodeid.split_once("::") else {
        return Err(PytestCollectError::NodeidNormalization {
            nodeid: nodeid.to_string(),
            message: "pytest nodeid must contain '::'".to_string(),
        });
    };
    let file_path = Path::new(file_part);
    let relative = if file_path.is_absolute() {
        file_path
            .strip_prefix(repo_root)
            .map_err(|_| PytestCollectError::NodeidNormalization {
                nodeid: nodeid.to_string(),
                message: format!("nodeid path is not under repo root {}", repo_root.display()),
            })?
    } else {
        file_path
    };
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PytestCollectError::NodeidNormalization {
            nodeid: nodeid.to_string(),
            message: "nodeid path must not contain '..'".to_string(),
        });
    }
    Ok(format!("{}::{}", posix_path(relative), rest))
}

fn posix_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;

    #[test]
    fn witness_collector_helpers() {
        assert_eq!(posix_path(Path::new("tests/a.py")), "tests/a.py");
        let payload = parse_collect_payload("KISS_COLLECT_JSON:{\"nodeids\":[]}").unwrap();
        assert!(payload.nodeids.is_empty());
        let config = collect_config_json(&[PathBuf::from("tests/t.py")], &["-q".into()]).unwrap();
        assert!(config.contains("tests/t.py"));
        let decoded: CollectConfigPayload = serde_json::from_str(&config).unwrap();
        assert_eq!(decoded.paths, vec!["tests/t.py".to_string()]);
        assert_eq!(
            CollectConfigPayload::witness().pytest_args,
            vec!["-q".to_string()]
        );
        let payload =
            parse_collect_payload("KISS_COLLECT_JSON:{\"nodeids\":[\"tests/a.py::t\"]}").unwrap();
        assert_eq!(payload.nodeids, vec!["tests/a.py::t".to_string()]);
        assert!(parse_collect_payload("KISS_COLLECT_JSON:{bad").is_err());
        let empty: CollectPayload = serde_json::from_str("{\"nodeids\":[]}").unwrap();
        assert!(empty.nodeids.is_empty());
        assert_eq!(
            CollectPayload::witness().nodeids,
            vec!["tests/a.py::t".to_string()]
        );
        assert!(is_empty_collection_success(Some(5), &empty));
        assert!(!is_empty_collection_success(Some(2), &empty));
        let _ = subprocess_pytest_collector();
        assert!(
            validate_collect_request(&PytestCollectRequest {
                cwd: PathBuf::from("."),
                python: PathBuf::from("python"),
                paths: Vec::new(),
                pytest_args: Vec::new(),
                env: BTreeMap::new(),
            })
            .is_ok()
        );
    }

    #[test]
    fn witness_collect_subprocess_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let tests = tmp.path().join("tests");
        fs::create_dir_all(&tests).unwrap();
        fs::write(
            tests.join("test_ok.py"),
            "def test_ok():\n    assert True\n",
        )
        .unwrap();
        let python =
            PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()));
        let request = PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: python.clone(),
            paths: vec![tests.join("test_ok.py")],
            pytest_args: vec!["-q".into()],
            env: BTreeMap::from([("KISS_COLLECT_ENV".into(), "1".into())]),
        };
        let success = collect_subprocess(request).unwrap();
        assert_eq!(
            success.nodeids,
            vec!["tests/test_ok.py::test_ok".to_string()]
        );

        let full_suite = collect_subprocess(PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: python.clone(),
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap();
        assert_eq!(full_suite.nodeids, success.nodeids);

        fs::write(
            tests.join("test_bad.py"),
            "import missing_module\n\ndef test_bad():\n    pass\n",
        )
        .unwrap();
        let failure = collect_subprocess(PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python,
            paths: vec![tests.join("test_bad.py")],
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap_err();
        assert!(matches!(
            failure,
            PytestCollectError::CollectionFailed { .. }
        ));
    }
}
