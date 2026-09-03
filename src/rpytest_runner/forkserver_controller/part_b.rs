pub(crate) const FORKSERVER_CONTROLLER_B: &str = r#"
class _TestDurationPlugin(object):
    def __init__(self):
        self.seconds = 0.0
        self.seen = False

    def pytest_runtest_logreport(self, report):
        # Call phase only: setup under rslip coverage includes import tracing
        # and must not dominate max_unit_test_seconds / PASS timing.
        if report.when == "call":
            self.seconds = float(report.duration)
            self.seen = True

def _write_duration(path, plugin):
    if not plugin.seen:
        return
    try:
        with open(os.path.abspath(path), "w", encoding="utf-8") as handle:
            handle.write("%.9f\n" % plugin.seconds)
    except Exception:
        pass

_TIMEOUT_HIT = []
_SETUP_WAIT_MS = 60000

def _timeout_handler(_signum, _frame):
    _TIMEOUT_HIT.append(1)
    print("pytest timed out", file=sys.stderr, flush=True)
    raise SystemExit(124)

def _arm_timeout(timeout_ms):
    _TIMEOUT_HIT.clear()
    if timeout_ms is None:
        return
    signal.signal(signal.SIGALRM, _timeout_handler)
    signal.setitimer(signal.ITIMER_REAL, max(timeout_ms / 1000.0, 0.001))

def _disarm_timeout():
    try:
        signal.setitimer(signal.ITIMER_REAL, 0)
    except Exception:
        pass

def _mark_gate(path):
    if not path:
        return
    try:
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("1\n")
    except Exception:
        pass

class _CallTimeoutPlugin(object):
    def __init__(self, timeout_ms, gate_path=None):
        self.timeout_ms = timeout_ms
        self.gate_path = gate_path

    def pytest_runtest_logreport(self, report):
        if report.when == "setup" and getattr(report, "passed", False):
            _mark_gate(self.gate_path)
            _arm_timeout(self.timeout_ms)
        elif report.when == "call":
            _disarm_timeout()

def _run_prepared_child(req, stdout_path, stderr_path, duration_path, gate_path):
    from _pytest.main import Session, ExitCode
    from _pytest.config.exceptions import UsageError
    from _pytest.outcomes import Failed, exit as pytest_exit
    import _pytest._code

    duration_plugin = _TestDurationPlugin()
    try:
        try:
            os.chdir(req["cwd"])
            os.environ.pop("PYTEST_ADDOPTS", None)
            # Do not set PYTEST_DISABLE_PLUGIN_AUTOLOAD here: nested pytest
            # invocations from tests need plugin autoload for pytest.ini addopts.
            os.environ.pop("PYTEST_DISABLE_PLUGIN_AUTOLOAD", None)
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

            for module_name in req.get("child_preload_modules", []):
                importlib.import_module(module_name)

            if _CONFIG is None:
                raise RuntimeError("controller was not bootstrapped")

            config = _CONFIG
            timeout_plugin = _CallTimeoutPlugin(req.get("timeout_ms"), gate_path)
            config.pluginmanager.register(duration_plugin, "rpytest_test_duration")
            config.pluginmanager.register(timeout_plugin, "rpytest_call_timeout")
            session = Session.from_config(config)
            session.exitstatus = ExitCode.OK
            initstate = 0
            try:
                try:
                    config.hook.pytest_sessionstart(session=session)
                    initstate = 2

                    def doit(_config, session):
                        items = session.perform_collect(args=[req["nodeid"]])
                        session.items = list(items)
                        if not session.items:
                            return ExitCode.NO_TESTS_COLLECTED
                        config.hook.pytest_runtestloop(session=session)
                        if session.testsfailed:
                            return ExitCode.TESTS_FAILED
                        if session.testscollected == 0:
                            return ExitCode.NO_TESTS_COLLECTED
                        return ExitCode.OK

                    session.exitstatus = doit(config, session) or 0
                except UsageError as exc:
                    session.exitstatus = ExitCode.USAGE_ERROR
                    sys.stderr.write("ERROR: %s\n" % exc)
                except Failed:
                    session.exitstatus = ExitCode.TESTS_FAILED
                except (KeyboardInterrupt, pytest_exit.Exception):
                    excinfo = _pytest._code.ExceptionInfo.from_current()
                    exitstatus = ExitCode.INTERRUPTED
                    if isinstance(excinfo.value, pytest_exit.Exception):
                        if excinfo.value.returncode is not None:
                            exitstatus = excinfo.value.returncode
                        if initstate < 2:
                            sys.stderr.write(
                                "%s: %s\n" % (excinfo.typename, excinfo.value.msg)
                            )
                    config.hook.pytest_keyboard_interrupt(excinfo=excinfo)
                    session.exitstatus = exitstatus
                except BaseException:
                    session.exitstatus = ExitCode.INTERNAL_ERROR
                    excinfo = _pytest._code.ExceptionInfo.from_current()
                    try:
                        config.notify_exception(excinfo, config.option)
                    except pytest_exit.Exception as exc:
                        if exc.returncode is not None:
                            session.exitstatus = exc.returncode
                        sys.stderr.write("%s: %s\n" % (type(exc).__name__, exc))
                    else:
                        if isinstance(excinfo.value, SystemExit):
                            sys.stderr.write(
                                "mainloop: caught unexpected SystemExit!\n"
                            )
            finally:
                os.chdir(session.startpath)
                if initstate >= 2:
                    try:
                        config.hook.pytest_sessionfinish(
                            session=session, exitstatus=session.exitstatus
                        )
                    except pytest_exit.Exception as exc:
                        if exc.returncode is not None:
                            session.exitstatus = exc.returncode
                        sys.stderr.write("%s: %s\n" % (type(exc).__name__, exc))
                try:
                    config.pluginmanager.unregister(timeout_plugin)
                except Exception:
                    pass
                try:
                    config.pluginmanager.unregister(duration_plugin)
                except Exception:
                    pass
                _disarm_timeout()
                _write_duration(duration_path, duration_plugin)
            raise SystemExit(int(session.exitstatus))
        except Exception:
            traceback.print_exc()
            _write_duration(duration_path, duration_plugin)
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

def _wait_status_after_gate(pid, timeout_ms, gate_path):
    if timeout_ms is None:
        return os.waitpid(pid, 0)[1], False
    setup_deadline = time.monotonic() + (_SETUP_WAIT_MS / 1000.0)
    while True:
        waited, status = os.waitpid(pid, os.WNOHANG)
        if waited != 0:
            return status, False
        if gate_path and os.path.exists(gate_path):
            break
        if time.monotonic() >= setup_deadline:
            try:
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass
            return os.waitpid(pid, 0)[1], True
        time.sleep(0.005)
    return _wait_status(pid, timeout_ms)

def _read_test_duration_ms(path):
    try:
        with open(path, "r", encoding="utf-8") as handle:
            text = handle.read().strip()
        if not text:
            return None
        seconds = float(text)
        if seconds < 0:
            return None
        return int(round(seconds * 1000.0))
    except Exception:
        return None

def _handle_run(req):
    stdout_fd, stdout_path = tempfile.mkstemp(prefix="rpytest-forkserver-out-")
    stderr_fd, stderr_path = tempfile.mkstemp(prefix="rpytest-forkserver-err-")
    duration_fd, duration_path = tempfile.mkstemp(prefix="rpytest-forkserver-dur-")
    os.close(stdout_fd)
    os.close(stderr_fd)
    os.close(duration_fd)
    gate_path = os.path.join(
        tempfile.gettempdir(),
        "rpytest-fs-gate-%s-%s" % (os.getpid(), time.time_ns()),
    )
    # Fork outside the cleanup try/finally: SystemExit in the child would
    # otherwise run that finally and delete duration_path before the parent reads it.
    pid = os.fork()
    if pid == 0:
        _run_prepared_child(
            req, stdout_path, stderr_path, duration_path, gate_path
        )
    try:
        status, forced_timeout = _wait_status_after_gate(
            pid, req.get("timeout_ms"), gate_path
        )
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
            "test_duration_ms": _read_test_duration_ms(duration_path),
        }
    finally:
        for path in (stdout_path, stderr_path, duration_path, gate_path):
            try:
                os.unlink(path)
            except FileNotFoundError:
                pass
"#;
