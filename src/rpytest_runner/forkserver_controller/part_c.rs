pub(crate) const FORKSERVER_CONTROLLER_C: &str = r#"
def _handle_run_module(req):
    # One fork per test. A shared child would keep module globals across tests.
    tests = list(req.get("tests") or [])
    if not tests:
        return {"results": [], "error": "run_module missing tests"}
    results = []
    shared_preload = list(req.get("child_preload_modules") or [])
    shared_cwd = req.get("cwd")
    try:
        for test in tests:
            test = dict(test)
            if shared_cwd and not test.get("cwd"):
                test["cwd"] = shared_cwd
            if not test.get("child_preload_modules"):
                test["child_preload_modules"] = list(shared_preload)
            result = _handle_run(test)
            results.append(result)
            _respond({"progress": result, "results": [], "error": None})
    except Exception as exc:
        return {"results": results, "error": "module batch failed: " + repr(exc)}
    return {"results": results, "error": None}

_OBS = set()
_EXT = False
_AUDITED = False

def _install_collect_audit():
    global _AUDITED
    if _AUDITED:
        return
    _AUDITED = True
    root = os.path.realpath(os.getcwd())

    def prefixes():
        out = {sys.prefix, getattr(sys, "base_prefix", sys.prefix), getattr(sys, "exec_prefix", sys.prefix)}
        try:
            import site
            out.update(site.getsitepackages())
            out.add(site.getusersitepackages())
        except Exception:
            pass
        return tuple(os.path.realpath(p) for p in out if p)

    pref = prefixes()

    def hook(event, args):
        global _EXT
        if event != "open" or not isinstance(args[0], str):
            return
        try:
            real = os.path.realpath(args[0])
        except Exception:
            return
        if real.startswith(root + os.sep) or real == root:
            if os.path.isfile(real):
                _OBS.add(os.path.relpath(real, root).replace("\\", "/"))
            return
        if any(real == p or real.startswith(p + os.sep) for p in pref):
            return
        if real.startswith(("/dev", "/proc", "/sys", "/tmp", "/var/tmp")):
            return
        _EXT = True

    sys.addaudithook(hook)

def _repo_nodeid(item):
    root = os.path.realpath(os.getcwd())
    raw = os.path.abspath(str(item.path))
    chosen = raw if os.path.islink(raw) else os.path.realpath(raw)
    rel = os.path.relpath(chosen, root).replace(os.sep, "/")
    nodeid = item.nodeid
    if "::" in nodeid:
        return rel + "::" + nodeid.split("::", 1)[1]
    return rel

def _handle_collect(req):
    import pytest
    out_buf, err_buf = io.StringIO(), io.StringIO()
    old_out, old_err = sys.stdout, sys.stderr
    try:
        sys.stdout, sys.stderr = out_buf, err_buf
        _install_collect_audit()
        from _pytest.main import Session
        if _CONFIG is None:
            raise RuntimeError("controller was not bootstrapped")
        paths = list(req.get("paths") or [])
        session = Session.from_config(_CONFIG)
        errors = []

        class _KissCollectErrors(object):
            def pytest_collectreport(self, report):
                if getattr(report, "failed", False):
                    errors.append(str(report.longrepr))

        _CONFIG.pluginmanager.register(_KissCollectErrors(), "kiss_collect_errors")
        _CONFIG.hook.pytest_sessionstart(session=session)
        try:
            items = list(session.perform_collect(args=paths if paths else None))
            if errors:
                raise RuntimeError("\n".join(errors))
        finally:
            try:
                _CONFIG.hook.pytest_sessionfinish(session=session, exitstatus=0)
            except Exception:
                pass
            for name in ("session", "funcmanage", "kiss_collect_errors"):
                plugin = _CONFIG.pluginmanager.get_plugin(name)
                if plugin is not None:
                    _CONFIG.pluginmanager.unregister(plugin)
        return {
            "op": "collect_result",
            "ok": True,
            "nodeids": [_repo_nodeid(item) for item in items],
            "observed_workspace": sorted(_OBS),
            "unsupported_external": bool(_EXT),
            "python_version": ".".join(map(str, sys.version_info[:3])),
            "pytest_version": pytest.__version__,
            "executable": sys.executable,
            "pytest_file": pytest.__file__,
            "error": None,
        }
    except Exception as exc:
        return {
            "op": "collect_result",
            "ok": False,
            "nodeids": [],
            "observed_workspace": sorted(_OBS),
            "unsupported_external": bool(_EXT),
            "python_version": "",
            "pytest_version": "",
            "executable": "",
            "pytest_file": "",
            "error": "collect failed: " + traceback.format_exc(),
        }
    finally:
        sys.stdout, sys.stderr = old_out, old_err

def _shutdown():
    global _CONFIG
    try:
        if _CONFIG is not None:
            _CONFIG._ensure_unconfigure()
            _CONFIG = None
    finally:
        _respond({"op": "shutdown_ack", "ok": True})
        raise SystemExit(0)

def _serve():
    _ensure_protocol_in()
    for line in _PROTOCOL_IN:
        request = None
        try:
            request = json.loads(line)
            op = request.get("op")
            if op == "bootstrap":
                _respond(_bootstrap(request))
            elif op == "shutdown":
                _shutdown()
            elif op == "run_module":
                _respond(_handle_run_module(request))
            elif op == "collect":
                _respond(_handle_collect(request))
            else:
                _respond(_handle_run(request))
        except Exception as exc:
            req = request or {}
            if req.get("op") == "bootstrap":
                _respond({
                    "op": "bootstrap_result",
                    "ok": False,
                    "error": "controller protocol error: " + repr(exc),
                    "stdout": [],
                    "stderr": [],
                })
            elif req.get("op") == "shutdown":
                _respond({"op": "shutdown_ack", "ok": False, "error": repr(exc)})
                raise SystemExit(1)
            else:
                _respond({
                    "id": req.get("id", 0),
                    "nodeid": req.get("nodeid", ""),
                    "status": "failed",
                    "exit_code": None,
                    "stdout": [],
                    "stderr": [],
                    "artifacts": {},
                    "timeout": False,
                    "error": "controller protocol error: " + repr(exc),
                })

def _die_with_parent():
    try:
        import ctypes
        libc = ctypes.CDLL(None)
        if libc.prctl(1, signal.SIGKILL) != 0:
            return
        if os.getppid() == 1:
            os.kill(os.getpid(), signal.SIGKILL)
    except Exception:
        return

def _zygote_main():
    os.environ["PYTEST_DISABLE_PLUGIN_AUTOLOAD"] = "1"
    _validate_pytest()
    _reject_non_main_threads()
    name = os.environ["KISS_FORKSERVER_ZYGOTE_SOCK"]
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        os.unlink(name)
    except OSError:
        pass
    srv.bind(name)
    srv.listen(128)
    signal.signal(signal.SIGCHLD, signal.SIG_IGN)
    sys.stdout.write(json.dumps({"op": "zygote_ready"}) + "\n")
    sys.stdout.flush()
    while True:
        conn, _addr = srv.accept()
        pid = os.fork()
        if pid != 0:
            conn.close()
            continue
        _die_with_parent()
        signal.signal(signal.SIGCHLD, signal.SIG_DFL)
        os.setsid()
        srv.close()
        os.environ.pop("KISS_FORKSERVER_MODE", None)
        os.environ.pop("KISS_FORKSERVER_ZYGOTE_SOCK", None)
        fd = conn.fileno()
        os.dup2(fd, 0)
        os.dup2(fd, 1)
        conn.close()
        global _PROTOCOL_IN
        _PROTOCOL_IN = os.fdopen(os.dup(0), "r", buffering=1)
        sys.stdout = os.fdopen(os.dup(1), "w", buffering=1)
        sys.stderr = open(os.devnull, "w", buffering=1)
        _respond({"op": "ready", "pid": os.getpid()})
        _serve()
        os._exit(0)

if os.environ.get("KISS_FORKSERVER_MODE") == "zygote":
    try:
        _zygote_main()
    except Exception:
        traceback.print_exc()
        raise SystemExit(1)
else:
    _serve()
"#;
