pub(crate) const FORKSERVER_CONTROLLER_C: &str = r#"
def _result_dict(req, exit_code, stdout, stderr, artifacts, timed_out, duration_ms, error=None):
    return {
        "id": req.get("id", 0),
        "nodeid": req.get("nodeid", ""),
        "status": "passed" if exit_code == 0 else "failed",
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "artifacts": artifacts,
        "timeout": timed_out,
        "error": error,
        "test_duration_ms": duration_ms,
    }

def _redirect_stdio(stdout_path, stderr_path):
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

def _reset_rslip_coverage(req):
    out_path = (req.get("env") or {}).get("RSLIP_COVERAGE_OUT")
    runtime = sys.modules.get("rslip_runtime")
    if runtime is None:
        return
    reset = getattr(runtime, "reset_coverage", None)
    if reset is not None:
        reset(out_path)

def _flush_rslip_coverage():
    runtime = sys.modules.get("rslip_runtime")
    if runtime is None:
        return
    write = getattr(runtime, "_write", None)
    if write is not None:
        write()

def _snapshot_rslip_coverage():
    runtime = sys.modules.get("rslip_runtime")
    snapshot = getattr(runtime, "snapshot_coverage", None) if runtime is not None else None
    return snapshot() if snapshot is not None else {}

def _merge_rslip_coverage(files):
    runtime = sys.modules.get("rslip_runtime")
    merge = getattr(runtime, "merge_coverage", None) if runtime is not None else None
    if merge is not None:
        merge(files)

def _run_item_inprocess(
    item, req, stdout_path, stderr_path, duration_path, collection_coverage
):
    from _pytest.runner import runtestprotocol

    duration_plugin = _TestDurationPlugin()
    os.chdir(req["cwd"])
    os.environ.pop("PYTEST_ADDOPTS", None)
    os.environ.pop("PYTEST_DISABLE_PLUGIN_AUTOLOAD", None)
    os.environ.update(req.get("env", {}))
    _apply_pythonpath_from_env()
    _redirect_stdio(stdout_path, stderr_path)
    timeout_plugin = _CallTimeoutPlugin(req.get("timeout_ms"))
    item.config.pluginmanager.register(timeout_plugin, "rpytest_call_timeout")
    exit_code = 1
    try:
        for module_name in req.get("child_preload_modules", []):
            loaded = sys.modules.get(module_name)
            if loaded is not None and module_name != "rslip_runtime":
                importlib.reload(loaded)
            else:
                importlib.import_module(module_name)
        _reset_rslip_coverage(req)
        reports = runtestprotocol(item, nextitem=None)
        failed = any(getattr(report, "failed", False) for report in reports)
        call_reports = [report for report in reports if getattr(report, "when", None) == "call"]
        if call_reports:
            duration_plugin.seconds = float(call_reports[0].duration)
            duration_plugin.seen = True
        exit_code = 1 if failed else 0
        if _TIMEOUT_HIT:
            exit_code = 124
    except SystemExit as exc:
        exit_code = 124 if _TIMEOUT_HIT else (exc.code if isinstance(exc.code, int) else 1)
    except Exception:
        traceback.print_exc()
        exit_code = 124 if _TIMEOUT_HIT else 1
    finally:
        try:
            item.config.pluginmanager.unregister(timeout_plugin)
        except Exception:
            pass
        _disarm_timeout()
        _merge_rslip_coverage(collection_coverage)
        _flush_rslip_coverage()
        _write_duration(duration_path, duration_plugin)
        try:
            sys.stdout.flush()
            sys.stderr.flush()
        except Exception:
            pass
    return exit_code

def _run_one_collected_result(item, test_req, shared_preload, collection_coverage):
    stdout_fd, stdout_path = tempfile.mkstemp(prefix="rpytest-forkserver-out-")
    stderr_fd, stderr_path = tempfile.mkstemp(prefix="rpytest-forkserver-err-")
    duration_fd, duration_path = tempfile.mkstemp(prefix="rpytest-forkserver-dur-")
    os.close(stdout_fd)
    os.close(stderr_fd)
    os.close(duration_fd)
    child_req = dict(test_req)
    if not child_req.get("child_preload_modules"):
        child_req["child_preload_modules"] = shared_preload
    try:
        exit_code = _run_item_inprocess(
            item, child_req, stdout_path, stderr_path, duration_path, collection_coverage
        )
        timed_out = child_req.get("timeout_ms") is not None and exit_code == 124
        artifacts = {a["name"]: a["path"] for a in child_req.get("artifacts", [])}
        return _result_dict(
            child_req,
            exit_code,
            _read_file(stdout_path),
            _read_file(stderr_path),
            artifacts,
            timed_out,
            _read_test_duration_ms(duration_path),
        )
    finally:
        for path in (stdout_path, stderr_path, duration_path):
            try:
                os.unlink(path)
            except FileNotFoundError:
                pass

def _run_module_in_child(req):
    from _pytest.main import Session, ExitCode

    tests = list(req.get("tests") or [])
    if _CONFIG is None:
        raise RuntimeError("controller was not bootstrapped")
    first = tests[0]
    os.chdir(req.get("cwd") or first.get("cwd"))
    os.environ.update(first.get("env") or {})
    _apply_pythonpath_from_env()
    preload = req.get("child_preload_modules", [])
    for module_name in preload:
        importlib.import_module(module_name)
    _reset_rslip_coverage(first)
    config = _CONFIG
    session = Session.from_config(config)
    session.exitstatus = ExitCode.OK
    nodeids = [test["nodeid"] for test in tests]
    config.hook.pytest_sessionstart(session=session)
    items = list(session.perform_collect(args=nodeids))
    collection_coverage = _snapshot_rslip_coverage()
    session.items = items
    by_nodeid = {item.nodeid: item for item in items}
    results = []
    # Attribute collection-only lines once per module. Repeating them on every
    # selector makes a module-level edit unnecessarily select every test.
    collection_pending = collection_coverage
    try:
        for test in tests:
            item = by_nodeid.get(test["nodeid"])
            if item is None:
                results.append(_result_dict(
                    test, 1, [], list(b"rpytest: collected item missing\n"),
                    {a["name"]: a["path"] for a in test.get("artifacts", [])},
                    False, None, "collected item missing",
                ))
                continue
            test = dict(test)
            test["cwd"] = req.get("cwd") or test.get("cwd")
            results.append(
                _run_one_collected_result(item, test, preload, collection_pending)
            )
            collection_pending = {}
            if results[-1].get("timeout"):
                break
    finally:
        try:
            config.hook.pytest_sessionfinish(session=session, exitstatus=session.exitstatus)
        except Exception:
            pass
    return results

def _module_wait_timeout_ms(tests):
    timeouts = [test.get("timeout_ms") for test in tests]
    if any(timeout is None for timeout in timeouts):
        return None
    return int(sum(timeouts) + _SETUP_WAIT_MS)

def _fork_module_chunk(req, tests):
    fd, path = tempfile.mkstemp(prefix="rpytest-forkserver-mod-")
    os.close(fd)
    chunk = dict(req)
    chunk["tests"] = tests
    pid = os.fork()
    if pid == 0:
        try:
            payload = {"results": _run_module_in_child(chunk), "error": None}
        except Exception as exc:
            payload = {"results": [], "error": "module collect failed: " + repr(exc)}
        try:
            with open(path, "w", encoding="utf-8") as handle:
                json.dump(payload, handle, separators=(",", ":"))
                handle.write("\n")
        except Exception:
            pass
        os._exit(0)
    try:
        _status, forced_timeout = _wait_status(pid, _module_wait_timeout_ms(tests))
        if forced_timeout:
            return {"results": [], "error": "module batch timed out"}
        try:
            with open(path, "r", encoding="utf-8") as handle:
                payload = json.load(handle)
        except Exception as exc:
            return {"results": [], "error": "module batch result missing: " + repr(exc)}
        if not isinstance(payload, dict):
            return {"results": [], "error": "module batch result invalid"}
        return {
            "results": payload.get("results") or [],
            "error": payload.get("error"),
        }
    finally:
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass

def _handle_run_module(req):
    tests = list(req.get("tests") or [])
    if not tests:
        return {"results": [], "error": "run_module missing tests"}
    all_results = []
    remaining = tests
    while remaining:
        payload = _fork_module_chunk(req, remaining)
        if payload.get("error"):
            return payload
        results = payload.get("results") or []
        if not results:
            return {"results": [], "error": "module batch result empty"}
        all_results.extend(results)
        if results[-1].get("timeout"):
            remaining = tests[len(all_results):]
            continue
        if len(all_results) != len(tests):
            return {"results": [], "error": "module batch result short"}
        remaining = []
    return {"results": all_results, "error": None}

def _shutdown():
    global _CONFIG
    try:
        if _CONFIG is not None:
            _CONFIG._ensure_unconfigure()
            _CONFIG = None
    finally:
        _respond({"op": "shutdown_ack", "ok": True})
        raise SystemExit(0)

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
"#;
