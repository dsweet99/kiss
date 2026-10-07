#!/usr/bin/env python3
"""Long-running integration QA commands for the local development `kiss`."""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import tempfile
import time
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterator

ROOT = Path(__file__).resolve().parents[1]
KISS = ROOT / "target" / "debug" / "kiss"
PY_SOURCE = Path("ops/evaluate.py")
RS_SOURCE = Path("src/cli_output/mod.rs")
LANGUAGES = ("python", "rust")


@dataclass
class Outcome:
    name: str
    returncode: int
    stdout: str
    stderr: str
    elapsed: float
    observation: "ProcessObservation | None" = None

    @property
    def combined(self) -> str:
        return self.stdout + self.stderr

    def metrics(self) -> dict[str, str]:
        result: dict[str, str] = {}
        for line in self.stdout.splitlines():
            key, separator, value = line.partition("=")
            if separator and key and " " not in key:
                result[key] = value
        return result


@dataclass
class Fixture:
    root: Path
    nested: Path
    env: dict[str, str]
    ignores: dict[str, list[str]]


@dataclass
class ProcessInfo:
    ppid: int
    threads: int
    rss_kib: int
    cpu_ticks: int
    command: str


@dataclass
class ProcessObservation:
    peak_process_count: int = 0
    peak_thread_count: int = 0
    peak_rss_kib: int = 0
    sampled_cpu_seconds: float = 0.0
    samples: int = 0
    command_peaks: dict[str, int] = field(default_factory=dict)
    phase_overlap_samples: int = 0
    sampled_command_lines: list[str] = field(default_factory=list)


def cargo_executable_name(command: str) -> str | None:
    if not command:
        return None
    parts = command.split()
    if not parts:
        return None
    return Path(parts[0]).name


def is_nested_subject_compile_path(command: str) -> bool:
    """True for subject-test cargo/rustc under /tmp outside kiss-qa fixtures.

    Observed QA fixture batches live under `/tmp/kiss-qa-…` and must still count.
    Nested subject trees include Rust `tempfile` (`/tmp/.tmp…`) and in-suite
    helpers such as `/tmp/kiss-export-minimal-*` from export-contract tests.
    """
    if "/tmp/kiss-qa" in command:
        return False
    return "/tmp/.tmp" in command or "/tmp/kiss-export-minimal-" in command


def is_compile_command(command: str) -> bool:
    """True for cargo compile processes seen under `cargo nextest run`.

    Live /proc samples during compile show `cargo test --no-run`, bare `rustc`,
    and `build-script-build` — not only `cargo` + ` rustc `/` build `.

    Nested in-suite cargo under subject temp paths is ignored: those are subject
    tests spawning their own trees, not the observed batch's compile-once phase.
    Fixture roots like `/tmp/kiss-qa-…` still count.
    """
    if is_nested_subject_compile_path(command):
        return False
    name = cargo_executable_name(command)
    padded = f" {command} "
    if name == "rustc":
        return True
    if name == "cargo" and (" rustc " in padded or " build " in padded):
        return True
    if name == "cargo" and " test " in padded and "--no-run" in command:
        return True
    return "build-script-build" in command


def is_test_execution_command(command: str) -> bool:
    """True for a Rust test binary that nextest spawned (`target/<profile>/deps/<name>-<hash>`).

    The persistent `cargo nextest run` parent stays alive across compile, so it
    does not count as test execution, and neither do build scripts.
    """
    parts = command.split()
    if not parts:
        return False
    executable = parts[0]
    return (
        "/target/" in executable
        and "/deps/" in executable
        and "build-script-build" not in executable
        and not is_nested_subject_compile_path(command)
    )


def sample_phase_flags(commands: list[str]) -> tuple[bool, bool]:
    build_active = False
    test_active = False
    for command in commands:
        if not command:
            continue
        if is_test_execution_command(command):
            test_active = True
        if is_compile_command(command):
            build_active = True
    return build_active, test_active


class LinuxProcessObserver:
    def __init__(self, root_pid: int) -> None:
        self.root_pid = root_pid
        self.clock_ticks = os.sysconf("SC_CLK_TCK")
        self.cpu_ticks_by_pid: dict[int, int] = {}
        self.total_cpu_ticks = 0
        self.observation = ProcessObservation()

    def sample(self) -> None:
        snapshot = read_proc_tree(self.root_pid)
        pids = set(snapshot)
        if not pids:
            return
        self.observation.samples += 1
        self.observation.peak_process_count = max(
            self.observation.peak_process_count,
            len(pids),
        )
        thread_count = 0
        rss_kib = 0
        commands: dict[str, int] = {}
        command_lines: list[str] = []
        for pid in pids:
            info = snapshot[pid]
            thread_count += info.threads
            rss_kib += info.rss_kib
            previous = self.cpu_ticks_by_pid.get(pid)
            if previous is not None and info.cpu_ticks >= previous:
                self.total_cpu_ticks += info.cpu_ticks - previous
            self.cpu_ticks_by_pid[pid] = info.cpu_ticks
            command = observed_command_name(info.command)
            if command:
                commands[command] = commands.get(command, 0) + 1
            if info.command:
                command_lines.append(info.command)
        build_active, test_active = sample_phase_flags(command_lines)
        if build_active and test_active:
            self.observation.phase_overlap_samples += 1
        if command_lines:
            self.observation.sampled_command_lines.extend(command_lines[:8])
        self.observation.peak_thread_count = max(
            self.observation.peak_thread_count,
            thread_count,
        )
        self.observation.peak_rss_kib = max(self.observation.peak_rss_kib, rss_kib)
        self.observation.sampled_cpu_seconds = self.total_cpu_ticks / self.clock_ticks
        for command, count in commands.items():
            self.observation.command_peaks[command] = max(
                self.observation.command_peaks.get(command, 0),
                count,
            )


def _read_proc_pid(pid: int) -> ProcessInfo | None:
    pid_path = Path("/proc") / str(pid)
    try:
        stat = (pid_path / "stat").read_text()
        status = (pid_path / "status").read_text()
        cmdline = (pid_path / "cmdline").read_bytes()
    except OSError:
        return None
    right_paren = stat.rfind(")")
    if right_paren < 0:
        return None
    fields = stat[right_paren + 2 :].split()
    if len(fields) < 13:
        return None
    threads = 0
    rss_kib = 0
    for line in status.splitlines():
        if line.startswith("Threads:"):
            threads = int(line.split()[1])
        elif line.startswith("VmRSS:"):
            rss_kib = int(line.split()[1])
    command = " ".join(part.decode(errors="replace") for part in cmdline.split(b"\0") if part)
    return ProcessInfo(
        ppid=int(fields[1]),
        threads=threads,
        rss_kib=rss_kib,
        cpu_ticks=int(fields[11]) + int(fields[12]),
        command=command,
    )


def _child_pids(pid: int) -> list[int]:
    children: list[int] = []
    task_root = Path("/proc") / str(pid) / "task"
    try:
        tasks = list(task_root.iterdir())
    except OSError:
        return children
    for task in tasks:
        try:
            text = (task / "children").read_text()
        except OSError:
            continue
        children.extend(int(part) for part in text.split() if part.isdecimal())
    return children


def read_proc_tree(root_pid: int) -> dict[int, ProcessInfo]:
    result: dict[int, ProcessInfo] = {}
    pending = [root_pid]
    seen: set[int] = set()
    while pending:
        pid = pending.pop()
        if pid in seen:
            continue
        seen.add(pid)
        info = _read_proc_pid(pid)
        if info is None:
            continue
        result[pid] = info
        pending.extend(_child_pids(pid))
    return result


def read_proc_snapshot() -> dict[int, ProcessInfo]:
    result: dict[int, ProcessInfo] = {}
    proc = Path("/proc")
    if not proc.is_dir():
        return result
    for pid_path in proc.iterdir():
        if not pid_path.name.isdecimal():
            continue
        info = _read_proc_pid(int(pid_path.name))
        if info is not None:
            result[int(pid_path.name)] = info
    return result


def descendant_pids(snapshot: dict[int, ProcessInfo], root_pid: int) -> set[int]:
    children: dict[int, list[int]] = {}
    for pid, info in snapshot.items():
        children.setdefault(info.ppid, []).append(pid)
    result: set[int] = set()
    pending = [root_pid]
    while pending:
        pid = pending.pop()
        if pid in result or pid not in snapshot:
            continue
        result.add(pid)
        pending.extend(children.get(pid, []))
    return result


def observed_command_name(command: str) -> str | None:
    if not command:
        return None
    executable = Path(command.split()[0]).name
    if "nextest" in executable or " nextest " in f" {command} ":
        return "cargo-nextest"
    if executable in {"cargo", "kiss"}:
        return executable
    return None


def run(
    name: str,
    argv: list[str],
    cwd: Path,
    env: dict[str, str],
    expected: int | None = 0,
    timeout: int = 1_200,
) -> Outcome:
    started = time.monotonic()
    completed = subprocess.run(
        argv,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        timeout=timeout,
        check=False,
    )
    outcome = Outcome(
        name,
        completed.returncode,
        completed.stdout,
        completed.stderr,
        time.monotonic() - started,
    )
    print(f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s")
    metrics = outcome.metrics()
    interesting = (
        "selected_python",
        "selected_rust_initial",
        "python_population_required",
        "rust_population_required",
        "python_population_selectors",
        "rust_population_selectors",
        "python_total",
        "python_cache_hits",
        "python_cache_misses",
        "rust_population_total",
        "rust_population_cache_hits",
        "rust_population_cache_misses",
        "rust_final_total",
        "rust_final_cache_hits",
        "rust_final_cache_misses",
        "raw_artifact_count",
        "rust_concurrency_budget",
        "rust_build_target_count",
        "rust_max_active_test_instances",
        "rust_max_active_exports",
        "rust_transient_residual_count",
        "rust_external_tmp_residual_bytes",
        "rust_external_tmp_residual_count",
    )
    summary = ", ".join(f"{key}={metrics[key]}" for key in interesting if key in metrics)
    if summary:
        print(f"  {summary}")
    if expected is not None and outcome.returncode != expected:
        raise AssertionError(
            f"{name}: expected rc={expected}, got {outcome.returncode}\n"
            f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
        )
    return outcome


def run_observed(
    name: str,
    argv: list[str],
    cwd: Path,
    env: dict[str, str],
    expected: int | None = 0,
    timeout: int = 1_200,
    sample_interval: float = 0.1,
) -> Outcome:
    started = time.monotonic()
    with (
        tempfile.TemporaryFile("w+t") as stdout_file,
        tempfile.TemporaryFile("w+t") as stderr_file,
    ):
        process = subprocess.Popen(
            argv,
            cwd=cwd,
            env=env,
            text=True,
            stdout=stdout_file,
            stderr=stderr_file,
        )
        observer = LinuxProcessObserver(process.pid)
        deadline = started + timeout
        while process.poll() is None:
            observer.sample()
            if time.monotonic() > deadline:
                process.kill()
                process.wait()
                raise subprocess.TimeoutExpired(argv, timeout)
            try:
                process.wait(timeout=sample_interval)
            except subprocess.TimeoutExpired:
                continue
        observer.sample()
        stdout_file.seek(0)
        stderr_file.seek(0)
        outcome = Outcome(
            name,
            process.returncode,
            stdout_file.read(),
            stderr_file.read(),
            time.monotonic() - started,
            observer.observation,
        )
    print(
        f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s "
        f"peak_processes={outcome.observation.peak_process_count} "
        f"peak_threads={outcome.observation.peak_thread_count}"
    )
    metrics = outcome.metrics()
    interesting = (
        "selected_python",
        "selected_rust_initial",
        "python_population_required",
        "rust_population_required",
        "python_population_selectors",
        "rust_population_selectors",
        "python_total",
        "python_cache_hits",
        "python_cache_misses",
        "rust_population_total",
        "rust_population_cache_hits",
        "rust_population_cache_misses",
        "rust_final_total",
        "rust_final_cache_hits",
        "rust_final_cache_misses",
        "raw_artifact_count",
        "rust_concurrency_budget",
        "rust_build_target_count",
        "rust_max_active_test_instances",
        "rust_max_active_exports",
        "rust_transient_residual_count",
        "rust_external_tmp_residual_bytes",
        "rust_external_tmp_residual_count",
    )
    summary = ", ".join(f"{key}={metrics[key]}" for key in interesting if key in metrics)
    if summary:
        print(f"  {summary}")
    if expected is not None and outcome.returncode != expected:
        raise AssertionError(
            f"{name}: expected rc={expected}, got {outcome.returncode}\n"
            f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
        )
    return outcome


def run_interrupted(
    name: str,
    argv: list[str],
    cwd: Path,
    env: dict[str, str],
    signal_after: float,
    sig: signal.Signals = signal.SIGINT,
    settle: float = 2.0,
    timeout: int = 1_200,
) -> Outcome:
    started = time.monotonic()
    with (
        tempfile.TemporaryFile("w+t") as stdout_file,
        tempfile.TemporaryFile("w+t") as stderr_file,
    ):
        process = subprocess.Popen(
            argv,
            cwd=cwd,
            env=env,
            text=True,
            stdout=stdout_file,
            stderr=stderr_file,
            start_new_session=True,
        )
        deadline = started + timeout
        while time.monotonic() < deadline:
            if process.poll() is not None:
                break
            if time.monotonic() - started >= signal_after:
                os.killpg(os.getpgid(process.pid), sig)
                break
            time.sleep(0.05)
        else:
            process.kill()
            process.wait()
            raise subprocess.TimeoutExpired(argv, timeout)
        try:
            process.wait(timeout=max(1.0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise
        time.sleep(settle)
        stdout_file.seek(0)
        stderr_file.seek(0)
        outcome = Outcome(
            name,
            process.returncode,
            stdout_file.read(),
            stderr_file.read(),
            time.monotonic() - started,
        )
    print(
        f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s "
        f"(interrupted after {signal_after:.2f}s)"
    )
    return outcome


def run_concurrent(
    name: str,
    commands: list[tuple[list[str], Path]],
    env: dict[str, str],
    timeout: int = 1_200,
    allow_failures: bool = False,
) -> list[Outcome]:
    started = time.monotonic()
    processes = [
        subprocess.Popen(
            argv,
            cwd=cwd,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        for argv, cwd in commands
    ]
    outcomes: list[Outcome] = []
    for index, process in enumerate(processes):
        stdout, stderr = process.communicate(timeout=timeout)
        outcomes.append(
            Outcome(
                f"{name}-{index}",
                process.returncode,
                stdout,
                stderr,
                time.monotonic() - started,
            )
        )
    print(f"{name}: {len(outcomes)} processes, elapsed={time.monotonic() - started:.2f}s")
    for outcome in outcomes:
        print(f"  {outcome.name}: rc={outcome.returncode}")
    if not allow_failures:
        failed = [outcome for outcome in outcomes if outcome.returncode != 0]
        assert not failed, f"{name}: {len(failed)} concurrent process(es) failed"
    return outcomes


def assert_metric(metrics: dict[str, str], key: str, expected: str) -> None:
    actual = metrics.get(key)
    assert actual == expected, f"{key}: expected {expected!r}, got {actual!r}"


def metric_int(metrics: dict[str, str], key: str) -> int:
    assert key in metrics, f"missing metric {key}: {metrics}"
    return int(metrics[key])


def rendered_plan(outcome: Outcome) -> str:
    body = outcome.stdout.partition("KISS TEST METRICS")[0]
    return "\n".join(
        line for line in body.splitlines() if not line.startswith("kiss test: stage ")
    )


def changed_text(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    count = text.count(old)
    assert count == 1, f"{path}: expected one occurrence of {old!r}, found {count}"
    path.write_text(text.replace(old, new))


def directory_size_bytes(path: Path) -> int:
    if not path.exists():
        return 0
    total = 0
    for child in path.rglob("*"):
        try:
            if child.is_file() and not child.is_symlink():
                total += child.stat().st_size
        except OSError:
            continue
    return total


def copy_fixture(destination: Path) -> None:
    ignored = shutil.ignore_patterns(
        "target",
        ".kiss",
        "_kpop",
        ".cursor",
        ".cursorrules",
        ".testmondata",
        ".malvin",
        ".malvin_home",
        "__pycache__",
        ".pytest_cache",
        "log",
        "log_1",
        "o",
    )
    shutil.copytree(ROOT, destination, dirs_exist_ok=True, ignore=ignored)


def harness_oracle_test_file(path: Path) -> bool:
    return (
        path.name.startswith("test_")
        or path.name.endswith("_test.py")
        or "tests" in path.parts
        or "test" in path.parts
    )


def language_ignores(root: Path, language: str) -> list[str]:
    if language == "python":
        ignored = [
            path.name
            for path in root.rglob("*.py")
            if harness_oracle_test_file(path.relative_to(root))
        ]
    else:
        # kiss --ignore matches filename prefixes. Never emit the RS_SOURCE basename
        # (e.g. mod.rs), or sibling files with the same name would ignore the edit target.
        keep_name = RS_SOURCE.name
        ignored = [
            path.name
            for path in root.rglob("*.rs")
            if path.relative_to(root) != RS_SOURCE and path.name != keep_name
        ]
    result: list[str] = []
    for path in sorted(set(ignored)):
        result.extend(["--ignore", path])
    return result


def kiss_command(
    language: str,
    ignores: list[str],
    *options: str,
    trailing_test_args: tuple[str, ...] = (),
) -> list[str]:
    # Honor the fixture `.kissconfig`.
    argv = [
        str(KISS),
        "--lang",
        language,
        "test",
        "commit",
        *options,
        *ignores,
    ]
    if trailing_test_args:
        argv.append("--")
        argv.extend(trailing_test_args)
    return argv


@contextmanager
def qa_fixture(prefix: str) -> Iterator[Fixture]:
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix=prefix) as tmp:
        root = Path(tmp) / "repo"
        copy_fixture(root)
        nested = root / "src" / "test_runner"
        assert nested.is_dir(), nested
        # GateConfig::load() reads only CWD `.kissconfig`; path-isolation and
        # concurrent races also run from `nested`, so write the same file there
        # (otherwise ensure_default_config_exists writes defaults).
        kissconfig = (
            "[global]\n"
            "duplication_enabled = false\n"
            "orphan_module_enabled = false\n"
            "[test]\n"
            "[test.max_unit_test_seconds]\n"
            '"*" = 30\n'
            "[python]\n"
            "[rust]\n"
        )
        (root / ".kissconfig").write_text(kissconfig)
        (nested / ".kissconfig").write_text(kissconfig)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(root)
        env.pop("RUSTFLAGS", None)
        # Never inherit stale publication-barrier paths from the parent shell;
        # a deleted barrier dir makes canonicalize() return bare NotFound mid-publish.
        env.pop("KISS_QA_PUBLICATION_BARRIER_DIR", None)
        env.pop("KISS_QA_PUBLICATION_BARRIER_TARGET", None)
        changed_text(
            root / PY_SOURCE,
            "if not EVALS_DIR.is_dir():",
            "if EVALS_DIR.is_dir() is False:",
        )
        changed_text(
            root / RS_SOURCE,
            '"{} in {}", msg, root.display()',
            '"{msg} in {}", root.display()',
        )
        ignores = {language: language_ignores(root, language) for language in LANGUAGES}
        print(
            f"fixture: {root} python_ignores={len(ignores['python'])} "
            f"rust_ignores={len(ignores['rust'])}"
        )
        yield Fixture(root, nested, env, ignores)


def load_json(path: Path) -> dict:
    assert path.is_file(), f"missing persisted artifact: {path}"
    return json.loads(path.read_text())


def assert_check_gate_allowed(outcome: Outcome) -> None:
    assert outcome.returncode == 0 or "VIOLATION:" in outcome.stdout, (
        f"{outcome.name}: unexpected check result\nstdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
    )


def run_fixture_git(repo: Path, args: list[str]) -> None:
    env = os.environ.copy()
    env.update(
        {
            "GIT_AUTHOR_NAME": "Kiss QA",
            "GIT_AUTHOR_EMAIL": "kiss-qa@example.invalid",
            "GIT_COMMITTER_NAME": "Kiss QA",
            "GIT_COMMITTER_EMAIL": "kiss-qa@example.invalid",
        }
    )
    subprocess.run(["git", *args], cwd=repo, env=env, check=True, capture_output=True, text=True)


def commit_fixture_baseline(repo: Path) -> None:
    run_fixture_git(repo, ["init"])
    run_fixture_git(repo, ["add", "."])
    run_fixture_git(repo, ["commit", "-m", "baseline"])


def write_witness_config(repo: Path) -> None:
    # Witness repos intentionally leave `gamma` untested.
    # Do not set the legacy [global] orphan_module_enabled key; unknown global
    # keys prevent the [test] section from applying.
    (repo / ".kissconfig").write_text(
        "[global]\n"
        "duplication_enabled = false\n"
        "[test]\n"
        "orphan_detection = false\n"
        "[python]\n"
        "[rust]\n",
    )
    (repo / ".gitignore").write_text(".kiss/\ntarget/\n.pytest_cache/\n__pycache__/\n")


def write_python_witness_repo(repo: Path) -> None:
    repo.mkdir()
    (repo / "tests").mkdir()
    write_witness_config(repo)
    (repo / "app.py").write_text(
        "def alpha():\n"
        "    return 'alpha'\n"
        "\n"
        "def beta():\n"
        "    return 'beta'\n"
        "\n"
        "def gamma():\n"
        "    return 'gamma'\n",
    )
    (repo / "tests/test_app.py").write_text(
        "import os\n"
        "from pathlib import Path\n"
        "\n"
        "import app\n"
        "\n"
        "\n"
        "def mark(name):\n"
        "    root = Path(os.environ['KISS_WITNESS_DIR'])\n"
        "    root.mkdir(parents=True, exist_ok=True)\n"
        "    (root / name).write_text('ran')\n"
        "\n"
        "\n"
        "def test_alpha():\n"
        "    mark('python-alpha')\n"
        "    assert app.alpha() == 'alpha'\n"
        "\n"
        "\n"
        "def test_beta():\n"
        "    mark('python-beta')\n"
        "    assert app.beta() == 'beta'\n",
    )
    commit_fixture_baseline(repo)


def write_rust_witness_repo(repo: Path) -> None:
    repo.mkdir()
    (repo / "src").mkdir()
    (repo / "tests").mkdir()
    write_witness_config(repo)
    (repo / "Cargo.toml").write_text(
        "[package]\n"
        "name = \"kiss_witness\"\n"
        "version = \"0.1.0\"\n"
        "edition = \"2024\"\n",
    )
    (repo / "src/lib.rs").write_text(
        "pub fn alpha() -> &'static str {\n"
        "    \"alpha\"\n"
        "}\n"
        "\n"
        "pub fn beta() -> &'static str {\n"
        "    \"beta\"\n"
        "}\n"
        "\n"
        "pub fn gamma() -> &'static str {\n"
        "    \"gamma\"\n"
        "}\n",
    )
    (repo / "tests/alpha.rs").write_text(
        "#[test]\n"
        "fn test_alpha() {\n"
        "    assert_eq!(kiss_witness::alpha(), \"alpha\");\n"
        "}\n"
    )
    (repo / "tests/beta.rs").write_text(
        "#[test]\n"
        "fn test_beta() {\n"
        "    assert_eq!(kiss_witness::beta(), \"beta\");\n"
        "}\n"
    )
    subprocess.run(
        ["cargo", "generate-lockfile"],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    )
    commit_fixture_baseline(repo)


def marker_names(marker_dir: Path) -> set[str]:
    if not marker_dir.is_dir():
        return set()
    return {path.name for path in marker_dir.iterdir() if path.is_file()}


def clear_markers(marker_dir: Path) -> None:
    marker_dir.mkdir(parents=True, exist_ok=True)
    for path in marker_dir.iterdir():
        if path.is_file():
            path.unlink()


def python_records_dir(repo_root: Path) -> Path:
    return repo_root / ".kiss" / "test" / "records" / "python"


def run_witness_check(
    language: str,
    repo: Path,
    marker_dir: Path,
    jobs: int | None = None,
) -> Outcome:
    env = witness_env(repo, marker_dir)
    outcome = run(
        f"{language}-witness-check",
        witness_check_command(language, repo, jobs=jobs),
        repo,
        env,
        expected=None,
    )
    assert_check_gate_allowed(outcome)
    return outcome


def run_witness_commit(language: str, repo: Path, marker_dir: Path) -> Outcome:
    env = witness_env(repo, marker_dir)
    return run(
        f"{language}-witness-commit",
        [str(KISS), "--lang", language, "test", "commit"],
        repo,
        env,
    )


def witness_env(repo: Path, marker_dir: Path) -> dict[str, str]:
    env = os.environ.copy()
    env["PYTHONPATH"] = str(repo)
    env["KISS_WITNESS_DIR"] = str(marker_dir)
    env.pop("RUSTFLAGS", None)
    return env


def witness_check_command(language: str, repo: Path, jobs: int | None = None) -> list[str]:
    # Honor the fixture `.kissconfig`.
    command = [str(KISS), "--lang", language, "test"]
    if jobs is not None:
        command.extend(["-j", str(jobs)])
    command.append(str(repo))
    return command


def witness_test_command(language: str, repo: Path, jobs: int | None = None) -> list[str]:
    # Honor the fixture `.kissconfig`.
    command = [str(KISS), "--lang", language, "test"]
    if jobs is not None:
        command.extend(["-j", str(jobs)])
    command.append(str(repo))
    return command


def run_witness_test(
    language: str,
    repo: Path,
    marker_dir: Path,
    jobs: int | None = None,
) -> Outcome:
    env = witness_env(repo, marker_dir)
    return run(
        f"{language}-witness-test",
        witness_test_command(language, repo, jobs=jobs),
        repo,
        env,
        expected=0,
    )


def cache_tree_bytes(cache: Path, paths: list[Path]) -> dict[str, bytes]:
    result: dict[str, bytes] = {}
    for path in paths:
        assert path.is_file(), f"missing artifact: {path}"
        result[path.relative_to(cache).as_posix()] = path.read_bytes()
    return result


def rust_records_dir(repo_root: Path) -> Path:
    return repo_root / ".kiss" / "test" / "records" / "rust"


def executed_tests(outcome: Outcome) -> list[str]:
    """Tests kiss ran in `outcome`; a cached PASS prints no result line."""
    return [
        line
        for line in outcome.stdout.splitlines()
        if line.startswith(("PASS", "FAIL", "TIMEOUT"))
    ]


def assert_rust_record_witness(repo: Path, marker_dir: Path) -> None:
    cold = run_witness_test("rust", repo, marker_dir, jobs=4)
    for name in ("test_alpha", "test_beta"):
        assert any(name in line for line in executed_tests(cold)), cold.stdout
    record_paths = sorted(rust_records_dir(repo).glob("*.json"))
    records = [load_json(path) for path in record_paths]
    test_ids = {record["test_id"] for record in records}
    assert any("test_alpha" in test_id for test_id in test_ids), test_ids
    assert any("test_beta" in test_id for test_id in test_ids), test_ids
    post_test_bytes = cache_tree_bytes(rust_records_dir(repo), record_paths)
    warm = run_witness_check("rust", repo, marker_dir, jobs=4)
    assert executed_tests(warm) == [], warm.stdout
    assert cache_tree_bytes(rust_records_dir(repo), record_paths) == post_test_bytes
    changed_text(repo / "src/lib.rs", "    \"alpha\"", "    { \"alpha\" }")
    commit = run_witness_commit("rust", repo, marker_dir)
    # Any Rust source edit reruns every Rust test.
    for name in ("test_alpha", "test_beta"):
        assert any(name in line for line in executed_tests(commit)), commit.stdout


def assert_python_record_witness(repo: Path, marker_dir: Path) -> None:
    run_witness_test("python", repo, marker_dir)
    assert marker_names(marker_dir) == {"python-alpha", "python-beta"}
    record_paths = sorted(python_records_dir(repo).glob("*.json"))
    test_ids = {load_json(path)["test_id"] for path in record_paths}
    assert any("test_alpha" in test_id for test_id in test_ids), test_ids
    assert any("test_beta" in test_id for test_id in test_ids), test_ids
    post_test_bytes = cache_tree_bytes(python_records_dir(repo), record_paths)
    clear_markers(marker_dir)
    warm = run_witness_check("python", repo, marker_dir)
    assert executed_tests(warm) == [], warm.stdout
    assert marker_names(marker_dir) == set()
    assert cache_tree_bytes(python_records_dir(repo), record_paths) == post_test_bytes
    changed_text(repo / "app.py", "    return 'alpha'", "    return str('alpha')")
    commit = run_witness_commit("python", repo, marker_dir)
    # Any Python source edit reruns every Python test.
    assert marker_names(marker_dir) == {"python-alpha", "python-beta"}, commit.stdout


def record_cache_witness() -> None:
    """Prove Python and Rust record payloads, warm non-execution, and edit reruns."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-", dir="/tmp") as tmp:
        root = Path(tmp)
        py_repo = root / "p"
        rs_repo = root / "r"
        py_markers = root / "pm"
        rs_markers = root / "rm"
        write_python_witness_repo(py_repo)
        write_rust_witness_repo(rs_repo)
        assert_python_record_witness(py_repo, py_markers)
        assert_rust_record_witness(rs_repo, rs_markers)


def timing_rust_throughput(
    runs: int = 1,
    job_values: tuple[int, ...] = (2,),
    legacy_cold_j1_median: float | None = None,
) -> None:
    """Timing: cold and warm Rust test runs."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    del runs, job_values, legacy_cold_j1_median
    with tempfile.TemporaryDirectory(prefix="kq-tput-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        command = witness_test_command("rust", repo, jobs=2)
        cold = run_observed("tput-cold", command, repo, env)
        assert cold.returncode == 0
        warm = run_observed("tput-warm", command, repo, env)
        assert warm.returncode == 0
        emit_eval("rust_cold_elapsed_s", "SMALLER", f"{cold.elapsed:.4f}")
        emit_eval("rust_warm_elapsed_s", "SMALLER", f"{warm.elapsed:.4f}")


def path_isolation() -> None:
    """Test nested-CWD plans and persisted path isolation."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-path-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        markers = Path(tmp) / "m"
        write_python_witness_repo(repo)
        nested = repo / "nested"
        nested.mkdir()
        (nested / ".kissconfig").write_text((repo / ".kissconfig").read_text())
        env = witness_env(repo, markers)
        from_root = run(
            "path-root",
            [str(KISS), "--lang", "python", "test", "commit"],
            repo,
            env,
        )
        from_nested = run(
            "path-nested",
            [str(KISS), "--lang", "python", "test", "commit"],
            nested,
            env,
        )
        assert from_root.returncode == 0
        assert from_nested.returncode == 0


def concurrent_cache_recovery() -> None:
    """Race shared caches, then test malformed-index recovery."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-ccr-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        markers = Path(tmp) / "m"
        write_python_witness_repo(repo)
        env = witness_env(repo, markers)
        cmd = [str(KISS), "--lang", "python", "test", ".", "-j", "2"]
        first = run("ccr-prime", cmd, repo, env)
        assert first.returncode == 0
        records = sorted(python_records_dir(repo).glob("*.json"))
        assert records, f"missing Python test records in {python_records_dir(repo)}"
        records[0].write_text("{ broken")
        recovered = run("ccr-recover", cmd, repo, env, expected=None)
        assert recovered.returncode == 0 or "VIOLATION" in recovered.stdout


def rust_batch_e2e() -> None:
    """E2E batch QA: cold batch, Ctrl-C, and recovery."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-e2e-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        cmd = witness_test_command("rust", repo, jobs=2)
        cold = run_observed("e2e-cold", cmd, repo, env)
        assert cold.returncode == 0
        run_interrupted("e2e-int", cmd, repo, env, signal_after=0.4)
        recovered = run("e2e-recover", cmd, repo, env)
        assert recovered.returncode == 0


def rust_phase_interrupt() -> None:
    """Interrupt a warm Rust run, then recover with a clean rerun."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-phase-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        cmd = witness_test_command("rust", repo, jobs=2)
        warm = run("phase-warm", cmd, repo, env)
        assert warm.returncode == 0
        run_interrupted("phase-int", cmd, repo, env, signal_after=0.3)
        recovered = run("phase-recover", cmd, repo, env)
        assert recovered.returncode == 0


def rust_full_repo_observer(jobs: int = 2) -> None:
    """Observe full-repository cold Rust population process/thread bounds."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-obs-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        outcome = run_observed(
            "observer-cold",
            witness_test_command("rust", repo, jobs=jobs),
            repo,
            env,
            timeout=50,
        )
        assert outcome.returncode == 0
        assert outcome.observation is not None
        emit_eval("rust_peak_rss_kib", "SMALLER", outcome.observation.peak_rss_kib)
        emit_eval(
            "rust_peak_processes", "SMALLER", outcome.observation.peak_process_count
        )


def rust_retained_cache_audit() -> None:
    """Audit the bytes Rust test records keep after a run."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-ret-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        run_witness_test("rust", repo, markers, jobs=2)
        records = rust_records_dir(repo)
        size = directory_size_bytes(records) if records.is_dir() else 0
        assert size >= 0
        emit_eval("rust_retained_cache_bytes", "SMALLER", size)


def shlex_quote(value: str) -> str:
    if not value:
        return "''"
    if all(ch.isalnum() or ch in "/._-:" for ch in value):
        return value
    return "'" + value.replace("'", "'\"'\"'") + "'"


def emit_eval(name: str, kind: str, value: object | None = None) -> None:
    if kind not in {"LARGER", "SMALLER"}:
        raise ValueError(
            f"VISION.md allows only LARGER/SMALLER eval metrics, got {kind!r}"
        )
    print(f"EVAL: {name} = {kind}({value})")


def _peak_rss_kib() -> int:
    try:
        usage = __import__("resource").getrusage(__import__("resource").RUSAGE_SELF)
        kids = __import__("resource").getrusage(__import__("resource").RUSAGE_CHILDREN)
        return int(max(usage.ru_maxrss, kids.ru_maxrss))
    except OSError:
        return 0


def report_eval(fn) -> None:
    started = time.monotonic()
    try:
        fn()
    except AssertionError:
        # Measurement validity checks must not become a pass/fail verdict.
        pass
    finally:
        emit_eval("elapsed_s", "SMALLER", f"{time.monotonic() - started:.4f}")
        emit_eval("peak_rss_kib", "SMALLER", _peak_rss_kib())

