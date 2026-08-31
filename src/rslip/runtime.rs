pub(crate) const MODULE_NAME: &str = "rslip_runtime";
pub(crate) const COVERAGE_ARTIFACT: &str = "line_coverage";

pub(crate) const PYTHON_RUNTIME: &str = r#"
import atexit
import json
import os
import sys

if sys.version_info < (3, 12):
    raise RuntimeError("rslip requires Python 3.12 or newer")

_out_path = os.environ["RSLIP_COVERAGE_OUT"]
_source_root = os.path.abspath(os.environ["RSLIP_SOURCE_ROOT"])
_files = {}
_tool_id = 4
_events = sys.monitoring.events.LINE


def _in_source_root(filename):
    if not filename or filename.startswith("<"):
        return False
    try:
        path = os.path.abspath(filename)
        return path == _source_root or path.startswith(_source_root + os.sep)
    except (TypeError, ValueError):
        return False


def _line_callback(code, line_number):
    filename = code.co_filename
    if _in_source_root(filename):
        _files.setdefault(os.path.abspath(filename), set()).add(int(line_number))
    return sys.monitoring.DISABLE


def _write():
    parent = os.path.dirname(_out_path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    payload = {"files": {path: sorted(lines) for path, lines in sorted(_files.items())}}
    tmp = _out_path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, sort_keys=True)
        handle.write("\n")
    os.replace(tmp, _out_path)


def reset_coverage(out_path=None):
    global _out_path, _files
    if out_path is not None:
        _out_path = out_path
    _files = {}
    try:
        sys.monitoring.restart_events()
    except Exception:
        pass


sys.monitoring.use_tool_id(_tool_id, "rslip")
sys.monitoring.register_callback(_tool_id, _events, _line_callback)
sys.monitoring.set_events(_tool_id, _events)
atexit.register(_write)
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_shim_uses_sys_monitoring_and_disable() {
        assert!(PYTHON_RUNTIME.contains("sys.monitoring.set_events"));
        assert!(PYTHON_RUNTIME.contains("sys.monitoring.DISABLE"));
        assert!(PYTHON_RUNTIME.contains("filename.startswith(\"<\")"));
        assert!(PYTHON_RUNTIME.contains("def reset_coverage"));
    }
}
