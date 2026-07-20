pub(crate) const FORKSERVER_CONTROLLER: &str = r#"
import importlib
import json
import os
import signal
import sys
import tempfile
import time
import traceback
import pytest

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

def _run_child(req, stdout_path, stderr_path):
    try:
        try:
            os.chdir(req["cwd"])
            os.environ.update(req.get("env", {}))
            _apply_pythonpath_from_env()
            stdout_fd = os.open(stdout_path, os.O_WRONLY | os.O_TRUNC)
            stderr_fd = os.open(stderr_path, os.O_WRONLY | os.O_TRUNC)
            os.dup2(stdout_fd, 1)
            os.dup2(stderr_fd, 2)
            os.close(stdout_fd)
            os.close(stderr_fd)
            sys.stdout = os.fdopen(os.dup(1), "w", buffering=1)
            sys.stderr = os.fdopen(os.dup(2), "w", buffering=1)
            sys.__stdout__ = sys.stdout
            sys.__stderr__ = sys.stderr

            timeout_ms = req.get("timeout_ms")
            if timeout_ms is not None:
                def _timeout(_signum, _frame):
                    print("pytest timed out", file=sys.stderr, flush=True)
                    raise SystemExit(124)
                signal.signal(signal.SIGALRM, _timeout)
                signal.setitimer(signal.ITIMER_REAL, max(timeout_ms / 1000.0, 0.001))

            for module_name in req.get("child_preload_modules", []):
                importlib.import_module(module_name)

            args = [req["nodeid"]] + list(req.get("pytest_args", []))
            raise SystemExit(pytest.main(args))
        except Exception:
            traceback.print_exc()
            raise SystemExit(1)
    finally:
        try:
            sys.stdout.flush()
            sys.stderr.flush()
        except Exception:
            pass

def _wait_status(pid, timeout_ms):
    if timeout_ms is None:
        return os.waitpid(pid, 0)[1], False
    deadline = time.monotonic() + max(float(timeout_ms) / 1000.0, 0.001)
    while True:
        waited, status = os.waitpid(pid, os.WNOHANG)
        if waited != 0:
            return status, False
        if time.monotonic() >= deadline:
            try:
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass
            return os.waitpid(pid, 0)[1], True
        time.sleep(0.005)

def _handle(req):
    stdout_fd, stdout_path = tempfile.mkstemp(prefix="rpytest-forkserver-out-")
    stderr_fd, stderr_path = tempfile.mkstemp(prefix="rpytest-forkserver-err-")
    os.close(stdout_fd)
    os.close(stderr_fd)
    try:
        pid = os.fork()
        if pid == 0:
            _run_child(req, stdout_path, stderr_path)
        status, forced_timeout = _wait_status(pid, req.get("timeout_ms"))
        if os.WIFEXITED(status):
            exit_code = os.WEXITSTATUS(status)
        elif os.WIFSIGNALED(status):
            exit_code = 128 + os.WTERMSIG(status)
        else:
            exit_code = 1
        timed_out = forced_timeout or (
            req.get("timeout_ms") is not None and exit_code == 124
        )
        artifacts = {a["name"]: a["path"] for a in req.get("artifacts", [])}
        return {
            "id": req["id"],
            "nodeid": req.get("nodeid", ""),
            "status": "passed" if exit_code == 0 else "failed",
            "exit_code": exit_code,
            "stdout": _read_file(stdout_path),
            "stderr": _read_file(stderr_path),
            "artifacts": artifacts,
            "timeout": timed_out,
            "error": None,
        }
    finally:
        for path in (stdout_path, stderr_path):
            try:
                os.unlink(path)
            except FileNotFoundError:
                pass

for line in sys.stdin:
    try:
        request = json.loads(line)
        _respond(_handle(request))
    except Exception as exc:
        _respond({
            "id": request.get("id", 0) if "request" in locals() else 0,
            "nodeid": request.get("nodeid", "") if "request" in locals() else "",
            "status": "failed",
            "exit_code": None,
            "stdout": [],
            "stderr": [],
            "artifacts": {},
            "timeout": False,
            "error": "controller protocol error: " + repr(exc),
        })
"#;
