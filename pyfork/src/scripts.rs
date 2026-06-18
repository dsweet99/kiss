pub fn pool_script() -> &'static str {
    r#"
import json, os, sys

def wait_one(running):
    done_pid, status = os.waitpid(-1, 0)
    code = os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else (status >> 8)
    nodeid = running.pop(done_pid)
    return nodeid, code

def run_pool(repo, nodeids, j, extra):
    import pytest
    sys.path.insert(0, repo)
    queue = list(nodeids)
    running = {}
    exit_codes = []
    while queue or running:
        while len(running) < j and queue:
            nodeid = queue.pop(0)
            pid = os.fork()
            if pid == 0:
                code = pytest.main([nodeid] + extra)
                os._exit(code if isinstance(code, int) else int(code))
            running[pid] = nodeid
        if running:
            nodeid, code = wait_one(running)
            if code != 0:
                print(f"=== {nodeid} ===", file=sys.stderr, flush=True)
            exit_codes.append(code)
    return max(exit_codes) if exit_codes else 0

def trace_one(repo, nodeid, out_path):
    import pytest
    os.environ["KISS_RSLIP"] = "1"
    sys.path.insert(0, repo)
    hits = {}
    collection_hits = {}
    current = None
    canonical_filenames = {}

    def tracer(frame, event, arg):
        nonlocal current
        if event == "line":
            raw_filename = frame.f_code.co_filename
            filename = canonical_filenames.get(raw_filename)
            if filename is None:
                filename = os.path.realpath(raw_filename)
                canonical_filenames[raw_filename] = filename
            if filename.startswith(repo + os.sep):
                rel = os.path.relpath(filename, repo).replace(os.sep, "/")
                target = collection_hits if current is None else hits.setdefault(current, {})
                target.setdefault(rel, set()).add(frame.f_lineno)
        return tracer

    class RslipPlugin:
        def pytest_runtest_setup(self, item):
            nonlocal current
            current = item.nodeid
        def pytest_runtest_teardown(self, item, nextitem):
            nonlocal current
            current = None

    sys.settrace(tracer)
    try:
        code = pytest.main([nodeid, "-q"], plugins=[RslipPlugin()])
    finally:
        sys.settrace(None)
    per_file = hits.get(nodeid, {})
    merged = {}
    for path in set(collection_hits) | set(per_file):
        merged[path] = sorted(set(collection_hits.get(path, set())) | set(per_file.get(path, set())))
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump({nodeid: merged}, fh)
    return code if isinstance(code, int) else int(code)

def trace_pool(repo, nodeids, j, trace_dir):
    queue = list(nodeids)
    running = {}
    exit_codes = []
    while queue or running:
        while len(running) < j and queue:
            nodeid = queue.pop(0)
            out_path = os.path.join(trace_dir, f"trace_{nodeid.replace('/', '_').replace(':', '_')}_{os.getpid()}.json")
            pid = os.fork()
            if pid == 0:
                code = trace_one(repo, nodeid, out_path)
                os._exit(code)
            running[pid] = nodeid
        if running:
            nodeid, code = wait_one(running)
            if code != 0:
                print(f"=== {nodeid} ===", file=sys.stderr, flush=True)
            exit_codes.append(code)
    return max(exit_codes) if exit_codes else 0

def main():
    repo = os.path.realpath(sys.argv[1])
    with open(sys.argv[2], encoding="utf-8") as fh:
        config = json.load(fh)
    mode = config.get("mode", "run")
    nodeids = config["nodeids"]
    j = int(config["j"])
    extra = config.get("extra", [])
    if mode == "trace":
        trace_dir = config["trace_dir"]
        os.makedirs(trace_dir, exist_ok=True)
        code = trace_pool(repo, nodeids, j, trace_dir)
    else:
        code = run_pool(repo, nodeids, j, extra)
    sys.exit(code)

if __name__ == "__main__":
    main()
"#
}

#[allow(dead_code)]
pub(crate) fn pid_once_script() -> &'static str {
    r#"
import json, os, sys
repo = os.path.realpath(sys.argv[1])
with open(sys.argv[2], encoding="utf-8") as fh:
    config = json.load(fh)
nodeid = config["nodeids"][0]
extra = config.get("extra", [])
import pytest
sys.path.insert(0, repo)
pid_path = config["pid_path"]
with open(pid_path, "w", encoding="utf-8") as fh:
    fh.write(str(os.getpid()))
code = pytest.main([nodeid] + extra)
sys.exit(code if isinstance(code, int) else int(code))
"#
}

#[allow(dead_code)]
pub(crate) fn slow_pool_script() -> &'static str {
    r#"
import json, os, sys, time

def wait_one(running):
    done_pid, status = os.waitpid(-1, 0)
    code = os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else (status >> 8)
    nodeid = running.pop(done_pid)
    return nodeid, code

def main():
    with open(sys.argv[2], encoding="utf-8") as fh:
        config = json.load(fh)
    nodeids = config["nodeids"]
    j = int(config["j"])
    sleep_s = float(config.get("sleep_s", 0.2))
    queue = list(nodeids)
    running = {}
    peak = 0
    peak_path = config["peak_path"]
    while queue or running:
        while len(running) < j and queue:
            nodeid = queue.pop(0)
            pid = os.fork()
            if pid == 0:
                time.sleep(sleep_s)
                os._exit(0)
            running[pid] = nodeid
            peak = max(peak, len(running))
            with open(peak_path, "w", encoding="utf-8") as fh:
                fh.write(str(peak))
        if running:
            wait_one(running)
    sys.exit(0)

if __name__ == "__main__":
    main()
"#
}
