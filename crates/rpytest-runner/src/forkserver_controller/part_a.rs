pub(crate) const FORKSERVER_CONTROLLER_A: &str = r#"
import importlib
import io
import json
import os
import signal
import sys
import tempfile
import threading
import time
import traceback

_CONFIG = None
_PROTOCOL_IN = os.fdopen(os.dup(0), "r", buffering=1)

def _respond(obj):
    sys.stdout.write(json.dumps(obj, separators=(",", ":")) + "\n")
    sys.stdout.flush()

def _read_file(path):
    try:
        with open(path, "rb") as f:
            return list(f.read())
    except FileNotFoundError:
        return []

def _apply_pythonpath_from_env():
    pythonpath = os.environ.get("PYTHONPATH")
    if not pythonpath:
        return
    for entry in reversed(pythonpath.split(os.pathsep)):
        if entry and entry not in sys.path:
            sys.path.insert(0, entry)

def _set_parent_env(env):
    os.environ.pop("PYTEST_ADDOPTS", None)
    os.environ["PYTEST_DISABLE_PLUGIN_AUTOLOAD"] = "1"
    os.environ.update(env or {})
    _apply_pythonpath_from_env()

def _validate_pytest8():
    import pytest
    from _pytest import config as pconfig
    from _pytest.main import Session
    major = int(pytest.__version__.split(".")[0])
    # Test-only override: inject a fake major without stubbing all of pytest.
    fake_major = os.environ.get("RPYTEST_FORKSERVER_FAKE_MAJOR")
    if fake_major is not None:
        major = int(fake_major)
    if major != 8:
        raise RuntimeError(
            "unsupported pytest major version %s; forkserver requires 8.x" % major
        )
    if not hasattr(pconfig, "_prepareconfig"):
        raise RuntimeError("pytest is missing _prepareconfig")
    if not hasattr(pconfig.Config, "_do_configure"):
        raise RuntimeError("pytest Config is missing _do_configure")
    if not hasattr(pconfig.Config, "_ensure_unconfigure"):
        raise RuntimeError("pytest Config is missing _ensure_unconfigure")
    if not hasattr(Session, "perform_collect"):
        raise RuntimeError("pytest Session is missing perform_collect")
    return pytest, pconfig, Session

def _reject_non_main_threads():
    alive = [
        t for t in threading.enumerate()
        if t is not threading.main_thread() and t.is_alive()
    ]
    if alive:
        names = ", ".join(t.name for t in alive)
        raise RuntimeError(
            "bootstrap left non-main Python threads alive (fork-unsafe): " + names
        )

def _bootstrap(boot):
    global _CONFIG
    out_buf, err_buf = io.StringIO(), io.StringIO()
    old_out, old_err = sys.stdout, sys.stderr
    try:
        sys.stdout, sys.stderr = out_buf, err_buf
        os.chdir(boot["cwd"])
        _set_parent_env(boot.get("env", {}))
        pytest, pconfig, _Session = _validate_pytest8()
        # Clear ini addopts (same as SubprocessPytestCollector): with plugin
        # autoload disabled, flags like --random-order are unrecognized. Keep
        # importlib mode so multi-path / shared-basename projects still work.
        # Explicit -p plugins still come from boot["pytest_args"].
        args = (
            ["-o", "addopts=", "--import-mode=importlib"]
            + list(boot.get("pytest_args", []))
            + ["."]
        )
        conf = pconfig._prepareconfig(args, plugins=[])
        conf._do_configure()
        _reject_non_main_threads()
        _CONFIG = conf
        # Nested pytest (shell'd from tests) must autoload plugins for pytest.ini
        # addopts such as --random-order. Keep autoload disabled only while
        # preparing this controller's config.
        os.environ.pop("PYTEST_DISABLE_PLUGIN_AUTOLOAD", None)
        # Pytest capture replaces sys.stdin with DontReadFromInput; keep protocol I/O
        # on the dup'd stream opened at startup.
        return {
            "op": "bootstrap_result",
            "ok": True,
            "error": None,
            "stdout": list(out_buf.getvalue().encode()),
            "stderr": list(err_buf.getvalue().encode()),
        }
    except Exception as exc:
        detail = repr(exc)
        try:
            cause = getattr(exc, "cause", None)
            if cause is not None:
                detail = detail + " cause=" + repr(cause)
        except Exception:
            pass
        return {
            "op": "bootstrap_result",
            "ok": False,
            "error": "bootstrap failed: " + detail,
            "stdout": list(out_buf.getvalue().encode()),
            "stderr": list(err_buf.getvalue().encode()) + list(
                traceback.format_exc().encode()
            ),
        }
    finally:
        sys.stdout, sys.stderr = old_out, old_err
"#;
