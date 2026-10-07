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
