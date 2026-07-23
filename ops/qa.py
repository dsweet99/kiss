#!/usr/bin/env python3
"""Long-running integration QA commands for the local development `kiss`."""

from __future__ import annotations

import json
import os
import shutil
import signal
import statistics
import subprocess
import tempfile
import time
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath
from typing import Iterator

import click

ROOT = Path(__file__).resolve().parents[1]
KISS = ROOT / "target" / "debug" / "kiss"
PY_SOURCE = Path("python/coverage_metrics.py")
PY_TEST = Path("tests/test_coverage_metrics_kiss.py")
RS_SOURCE = Path("src/cli_output.rs")
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
    llvm_single_thread_violations: int = 0
    build_jobs_mismatch: bool = False
    observed_build_jobs: int | None = None
    sampled_command_lines: list[str] = field(default_factory=list)


def llvm_tool_uses_single_thread(command: str) -> bool:
    if "llvm-cov" in command and " export" in f" {command} ":
        return "--threads=1" in command
    if "llvm-profdata" in command and " merge" in f" {command} ":
        return "--num-threads=1" in command
    return True


def cargo_build_jobs_from_command(command: str) -> int | None:
    parts = command.split()
    for index, part in enumerate(parts):
        if part == "--build-jobs" and index + 1 < len(parts):
            return int(parts[index + 1])
        if part.startswith("--build-jobs="):
            return int(part.split("=", 1)[1])
    return None


def cargo_executable_name(command: str) -> str | None:
    if not command:
        return None
    parts = command.split()
    if not parts:
        return None
    return Path(parts[0]).name


def sample_phase_flags(commands: list[str]) -> tuple[bool, bool, bool]:
    export_active = False
    test_active = False
    build_active = False
    for command in commands:
        if not command:
            continue
        if "llvm-cov" in command and " export" in f" {command} ":
            export_active = True
        if "llvm-profdata" in command and " merge" in f" {command} ":
            export_active = True
        if "llvm-cov nextest" in command:
            test_active = True
        if cargo_executable_name(command) == "cargo" and (
            " rustc " in f" {command} " or " build " in f" {command} "
        ):
            build_active = True
    return build_active, test_active, export_active


class LinuxProcessObserver:
    def __init__(self, root_pid: int) -> None:
        self.root_pid = root_pid
        self.clock_ticks = os.sysconf("SC_CLK_TCK")
        self.cpu_ticks_by_pid: dict[int, int] = {}
        self.total_cpu_ticks = 0
        self.observation = ProcessObservation()

    def sample(self) -> None:
        snapshot = read_proc_snapshot()
        pids = descendant_pids(snapshot, self.root_pid)
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
                if not llvm_tool_uses_single_thread(info.command):
                    self.observation.llvm_single_thread_violations += 1
                build_jobs = cargo_build_jobs_from_command(info.command)
                if build_jobs is not None:
                    self.observation.observed_build_jobs = build_jobs
        build_active, test_active, export_active = sample_phase_flags(command_lines)
        if (build_active and test_active) or (build_active and export_active) or (
            test_active and export_active
        ):
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


@dataclass
class ThroughputSample:
    jobs: int
    phase: str
    outcome: Outcome
    cache_bytes: int


def read_proc_snapshot() -> dict[int, ProcessInfo]:
    result: dict[int, ProcessInfo] = {}
    proc = Path("/proc")
    if not proc.is_dir():
        return result
    for pid_path in proc.iterdir():
        if not pid_path.name.isdecimal():
            continue
        pid = int(pid_path.name)
        try:
            stat = (pid_path / "stat").read_text()
            status = (pid_path / "status").read_text()
            cmdline = (pid_path / "cmdline").read_bytes()
        except OSError:
            continue
        right_paren = stat.rfind(")")
        if right_paren < 0:
            continue
        fields = stat[right_paren + 2 :].split()
        if len(fields) < 13:
            continue
        threads = 0
        rss_kib = 0
        for line in status.splitlines():
            if line.startswith("Threads:"):
                threads = int(line.split()[1])
            elif line.startswith("VmRSS:"):
                rss_kib = int(line.split()[1])
        command = " ".join(part.decode(errors="replace") for part in cmdline.split(b"\0") if part)
        result[pid] = ProcessInfo(
            ppid=int(fields[1]),
            threads=threads,
            rss_kib=rss_kib,
            cpu_ticks=int(fields[11]) + int(fields[12]),
            command=command,
        )
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
    if executable == "cargo" and " llvm-cov " in f" {command} ":
        return "cargo-llvm-cov"
    if "nextest" in executable or " nextest " in f" {command} ":
        return "cargo-nextest"
    if executable in {"cargo", "llvm-profdata", "llvm-cov", "kiss"}:
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
    click.echo(f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s")
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
        click.echo(f"  {summary}")
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
            time.sleep(sample_interval)
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
    click.echo(
        f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s "
        f"peak_processes={outcome.observation.peak_process_count} "
        f"peak_threads={outcome.observation.peak_thread_count}"
    )
    if expected is not None and outcome.returncode != expected:
        raise AssertionError(
            f"{name}: expected rc={expected}, got {outcome.returncode}\n"
            f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
        )
    return outcome


def lingering_processes_matching(substrings: tuple[str, ...]) -> list[str]:
    snapshot = read_proc_snapshot()
    matches: list[str] = []
    for pid, info in snapshot.items():
        if pid <= 1:
            continue
        command = info.command
        if command and all(part in command for part in substrings):
            matches.append(f"pid={pid} {command}")
    return matches


TARGET_RUNNER_SHIM_MARKER = "__rust-llvm-cov-target-runner"
DELEGATED_CHILD_MARKERS = (
    "KISS_RUST_LLVM_COV_DELEGATED_GO",
    "while [ ! -f",
)


def process_pgid(pid: int) -> int | None:
    try:
        return os.getpgid(pid)
    except ProcessLookupError:
        return None


def identity_still_valid(pid: int, pgid: int) -> bool:
    if pid <= 0 or pgid <= 0:
        return False
    try:
        os.kill(pid, 0)
        return os.getpgid(pid) == pgid
    except ProcessLookupError:
        return False


def live_shim_roles_from_metadata(repo_root: Path) -> dict[str, int]:
    cache_root = repo_root / ".kiss/rust_llvm_cov_cache"
    roles: dict[str, int] = {}
    if not cache_root.is_dir():
        return roles
    for start_path in sorted(cache_root.glob("runs/*/instances/*.shim-start.json")):
        try:
            metadata = json.loads(start_path.read_text())
            identity = metadata["shim_identity"]
            pid = int(identity["pid"])
            pgid = int(identity["pgid"])
        except (KeyError, TypeError, ValueError, json.JSONDecodeError, OSError):
            continue
        if identity_still_valid(pid, pgid):
            roles["shim"] = pgid
            break
    for start_path in sorted(cache_root.glob("runs/*/instances/*.delegated-start.json")):
        try:
            metadata = json.loads(start_path.read_text())
            identity = metadata["delegated_identity"]
            pid = int(identity["pid"])
            pgid = int(identity["pgid"])
        except (KeyError, TypeError, ValueError, json.JSONDecodeError, OSError):
            continue
        if identity_still_valid(pid, pgid):
            roles["delegated"] = pgid
            break
    return roles


def classify_batch_descendant_role(command: str) -> str | None:
    if not command:
        return None
    if TARGET_RUNNER_SHIM_MARKER in command:
        return "shim"
    if any(marker in command for marker in DELEGATED_CHILD_MARKERS):
        return "delegated"
    if "llvm-cov nextest" in command or " cargo nextest " in f" {command} ":
        return "nextest"
    executable = command.split()[0] if command.split() else ""
    if executable and "/target/" in executable:
        if not any(
            token in executable for token in ("cargo", "nextest", "kiss", "rustc")
        ):
            return "delegated"
    return None


def distinct_live_process_groups(
    root_pid: int,
    repo_root: Path | None = None,
) -> dict[str, int] | None:
    snapshot = read_proc_snapshot()
    roles: dict[str, int] = {}
    for pid in descendant_pids(snapshot, root_pid):
        if pid == root_pid:
            continue
        info = snapshot.get(pid)
        if info is None:
            continue
        role = classify_batch_descendant_role(info.command)
        if role is None:
            continue
        pgid = process_pgid(pid)
        if pgid is None:
            continue
        roles[role] = pgid
    if repo_root is not None:
        roles.update(live_shim_roles_from_metadata(repo_root))
    required = {"nextest", "shim", "delegated"}
    if not required.issubset(roles.keys()):
        return None
    if len({roles["nextest"], roles["shim"], roles["delegated"]}) != 3:
        return None
    return roles


def run_interrupt_after_distinct_live_groups(
    name: str,
    argv: list[str],
    cwd: Path,
    env: dict[str, str],
    settle: float = 2.0,
    timeout: int = 1_200,
    repo_root: Path | None = None,
) -> tuple[Outcome, dict[str, int]]:
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
        observer = LinuxProcessObserver(process.pid)
        live_groups: dict[str, int] | None = None
        test_phase_seen = False
        deadline = started + timeout
        while process.poll() is None and time.monotonic() < deadline:
            observer.sample()
            snapshot = read_proc_snapshot()
            command_lines = [
                info.command
                for pid in descendant_pids(snapshot, process.pid)
                if pid != process.pid
                for info in [snapshot.get(pid)]
                if info is not None and info.command
            ]
            _, test_active, export_active = sample_phase_flags(command_lines)
            if test_active and not export_active:
                test_phase_seen = True
                live_groups = distinct_live_process_groups(
                    process.pid,
                    repo_root=repo_root,
                )
                if live_groups is not None:
                    os.killpg(os.getpgid(process.pid), signal.SIGINT)
                    break
            time.sleep(0.05)
        else:
            if process.poll() is None:
                process.kill()
                process.wait()
                raise AssertionError(
                    f"{name}: timed out before distinct nextest/shim/delegated "
                    f"process groups were all live during test phase"
                )
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
            observer.observation,
        )
    if not test_phase_seen:
        raise AssertionError(f"{name}: test phase never became active")
    if live_groups is None:
        raise AssertionError(
            f"{name}: interrupted without recording distinct live process groups"
        )
    click.echo(
        f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s "
        f"nextest_pgid={live_groups['nextest']} "
        f"shim_pgid={live_groups['shim']} "
        f"delegated_pgid={live_groups['delegated']}"
    )
    return outcome, live_groups


def run_interrupt_on_phase(
    name: str,
    argv: list[str],
    cwd: Path,
    env: dict[str, str],
    target_phase: str,
    timeout: int = 1_200,
    settle: float = 2.0,
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
        observer = LinuxProcessObserver(process.pid)
        signaled = False
        deadline = started + timeout
        while process.poll() is None and time.monotonic() < deadline:
            observer.sample()
            build_active, test_active, export_active = sample_phase_flags(
                observer.observation.sampled_command_lines
            )
            phase_active = {
                "build": build_active and not test_active and not export_active,
                "test": test_active and not export_active,
                "export": export_active and not build_active and not test_active,
            }.get(target_phase, False)
            if phase_active and not signaled:
                os.killpg(os.getpgid(process.pid), signal.SIGINT)
                signaled = True
            time.sleep(0.05)
        if process.poll() is None:
            if not signaled:
                os.killpg(os.getpgid(process.pid), signal.SIGINT)
            try:
                process.wait(timeout=max(1.0, deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        time.sleep(settle)
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
    click.echo(
        f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s "
        f"phase={target_phase} signaled={signaled}"
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
    click.echo(
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
    click.echo(f"{name}: {len(outcomes)} processes, elapsed={time.monotonic() - started:.2f}s")
    for outcome in outcomes:
        click.echo(f"  {outcome.name}: rc={outcome.returncode}")
        if outcome.returncode != 0:
            click.echo(outcome.stdout)
            click.echo(outcome.stderr, err=True)
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
    return outcome.stdout.partition("KISS TEST METRICS")[0]


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


def test_file(path: Path) -> bool:
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
            if test_file(path.relative_to(root)) and path.relative_to(root) != PY_TEST
        ]
    else:
        ignored = [
            path.name for path in root.rglob("*.rs") if path.relative_to(root) != RS_SOURCE
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
    argv = [
        str(KISS),
        "--defaults",
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


def validate_command(language: str, ignores: list[str], jobs: int) -> list[str]:
    return [
        str(KISS),
        "--defaults",
        "--lang",
        language,
        "test",
        "validate-selection",
        "commit",
        "--dry-run",
        "-j",
        str(jobs),
        *ignores,
    ]


@contextmanager
def qa_fixture(prefix: str) -> Iterator[Fixture]:
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix=prefix) as tmp:
        root = Path(tmp) / "repo"
        copy_fixture(root)
        nested = root / "src" / "test_runner"
        assert nested.is_dir(), nested
        env = os.environ.copy()
        env["PYTHONPATH"] = str(root)
        env.pop("RUSTFLAGS", None)
        changed_text(
            root / PY_SOURCE,
            "return {path: partial.get(path, 0.0) for path in files}",
            "return {path: partial.get(path, float(0)) for path in files}",
        )
        changed_text(root / RS_SOURCE, "if file_pct >= 100 {", "if 100 <= file_pct {")
        ignores = {language: language_ignores(root, language) for language in LANGUAGES}
        click.echo(
            f"fixture: {root} python_ignores={len(ignores['python'])} "
            f"rust_ignores={len(ignores['rust'])}"
        )
        yield Fixture(root, nested, env, ignores)


def load_json(path: Path) -> dict:
    assert path.is_file(), f"missing persisted artifact: {path}"
    return json.loads(path.read_text())


def parse_rust_aggregate_refresh(stderr: str) -> tuple[int, int] | None:
    prefix = "kiss cov: refreshed Rust runtime coverage "
    for line in stderr.splitlines():
        if not line.startswith(prefix):
            continue
        fields = line.removeprefix(prefix).split()
        values: dict[str, int] = {}
        for item in fields:
            key, separator, value = item.partition("=")
            if separator:
                values[key] = int(value)
        if "rust_aggregate_binaries" in values and "rust_aggregate_exports" in values:
            return values["rust_aggregate_binaries"], values["rust_aggregate_exports"]
    return None


def assert_check_gate_allowed(outcome: Outcome) -> None:
    assert outcome.returncode == 0 or "GATE_FAILED:test_coverage" in outcome.stdout, (
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
    (repo / ".kissconfig").write_text(
        "[gate]\n"
        "test_coverage_threshold = 100\n"
        "duplication_enabled = false\n"
        "orphan_module_enabled = false\n",
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
        "    root = Path(os.environ['KISS_COVERAGE_WITNESS_DIR'])\n"
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
        "name = \"kiss_coverage_witness\"\n"
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
    rust_test = (
        "use std::fs;\n"
        "use std::path::PathBuf;\n"
        "\n"
        "fn mark(name: &str) {\n"
        "    let root = PathBuf::from(std::env::var(\"KISS_COVERAGE_WITNESS_DIR\").unwrap());\n"
        "    fs::create_dir_all(&root).unwrap();\n"
        "    fs::write(root.join(name), b\"ran\").unwrap();\n"
        "}\n"
    )
    (repo / "tests/alpha.rs").write_text(
        rust_test
        + "\n"
        + "#[test]\n"
        + "fn test_alpha() {\n"
        + "    mark(\"rust-alpha\");\n"
        + "    assert_eq!(kiss_coverage_witness::alpha(), \"alpha\");\n"
        + "}\n",
    )
    (repo / "tests/beta.rs").write_text(
        rust_test
        + "\n"
        + "#[test]\n"
        + "fn test_beta() {\n"
        + "    mark(\"rust-beta\");\n"
        + "    assert_eq!(kiss_coverage_witness::beta(), \"beta\");\n"
        + "}\n",
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


def relevant_artifact_bytes(paths: list[Path]) -> dict[str, bytes]:
    result: dict[str, bytes] = {}
    for path in paths:
        assert path.is_file(), f"missing artifact: {path}"
        result[path.name] = path.read_bytes()
    return result


def selector_entry_payloads(cache_root: Path) -> list[dict]:
    entries = sorted((cache_root / "entries").glob("*.json"))
    assert entries, f"missing selector entries in {cache_root}"
    return [load_json(path) for path in entries]


def entry_lines(entry: dict, source: str) -> set[int]:
    files = entry.get("coverage", {}).get("files", {})
    matched: set[int] = set()
    suffix = f"/{source}"
    for path, lines in files.items():
        if path == source or str(path).endswith(suffix):
            matched.update(int(line) for line in lines)
    return matched


def assert_index_source_selectors(
    index: dict,
    source: str,
    expected_parts: tuple[str, str],
) -> None:
    assert source in index["files"], f"{source} missing from index"
    selectors = index["files"][source]
    for part in expected_parts:
        assert any(part in selector for selector in selectors), selectors


def assert_disjoint_entry_lines(
    entries: list[dict],
    source: str,
    first_line: int,
    second_line: int,
    uncovered_line: int,
) -> None:
    first_entries = [entry for entry in entries if first_line in entry_lines(entry, source)]
    second_entries = [entry for entry in entries if second_line in entry_lines(entry, source)]
    uncovered_entries = [
        entry for entry in entries if uncovered_line in entry_lines(entry, source)
    ]
    assert len(first_entries) == 1, [entry_lines(entry, source) for entry in entries]
    assert len(second_entries) == 1, [entry_lines(entry, source) for entry in entries]
    assert first_entries[0] is not second_entries[0], "covered lines must be disjoint"
    assert not uncovered_entries, [entry_lines(entry, source) for entry in entries]


def assert_population_selectors(manifest: dict, expected_parts: tuple[str, str]) -> None:
    selectors = manifest["selectors"]
    for part in expected_parts:
        assert any(part in selector for selector in selectors), selectors


def assert_dry_run_selects_exactly(
    outcome: Outcome,
    expected_part: str,
    excluded_part: str,
) -> None:
    plan = rendered_plan(outcome)
    assert expected_part in plan, plan
    assert excluded_part not in plan, plan
    assert "PASSED:" not in plan and "FAILED:" not in plan, plan


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


def run_witness_dry_run(language: str, repo: Path, marker_dir: Path) -> Outcome:
    env = witness_env(repo, marker_dir)
    env.pop("RUSTFLAGS", None)
    return run(
        f"{language}-witness-dry-run",
        [str(KISS), "--defaults", "--lang", language, "test", "commit", "--dry-run", "--metrics"],
        repo,
        env,
    )


def witness_env(repo: Path, marker_dir: Path) -> dict[str, str]:
    env = os.environ.copy()
    env["PYTHONPATH"] = str(repo)
    env["KISS_COVERAGE_WITNESS_DIR"] = str(marker_dir)
    env.pop("RUSTFLAGS", None)
    return env


def witness_check_command(language: str, repo: Path, jobs: int | None = None) -> list[str]:
    command = [str(KISS), "--defaults", "--lang", language, "cov"]
    if jobs is not None:
        command.extend(["-j", str(jobs)])
    command.append(str(repo))
    return command


def assert_python_coverage_witness(repo: Path, marker_dir: Path) -> None:
    run_witness_check("python", repo, marker_dir)
    assert marker_names(marker_dir) == {"python-alpha", "python-beta"}
    cache = python_rslip_cache_root(repo)
    entry_payloads = selector_entry_payloads(cache)
    assert_disjoint_entry_lines(entry_payloads, "app.py", 2, 5, 8)
    index = load_json(cache / "index.json")
    manifest = load_json(cache / "population.json")
    assert_index_source_selectors(index, "app.py", ("test_alpha", "test_beta"))
    assert_population_selectors(manifest, ("test_alpha", "test_beta"))
    artifact_paths = sorted((cache / "entries").glob("*.json")) + [
        cache / "index.json",
        cache / "population.json",
    ]
    cold_bytes = relevant_artifact_bytes(artifact_paths)
    clear_markers(marker_dir)
    warm = run_witness_check("python", repo, marker_dir)
    assert "refreshing Python runtime coverage" not in warm.stderr, warm.stderr
    assert marker_names(marker_dir) == set()
    assert relevant_artifact_bytes(artifact_paths) == cold_bytes
    changed_text(repo / "app.py", "    return 'alpha'", "    return str('alpha')")
    dry = run_witness_dry_run("python", repo, marker_dir)
    assert_dry_run_selects_exactly(dry, "test_alpha", "test_beta")


def assert_rust_coverage_witness(repo: Path, marker_dir: Path) -> None:
    cold = run_witness_check("rust", repo, marker_dir, jobs=4)
    assert marker_names(marker_dir) == {"rust-alpha", "rust-beta"}
    cache = repo / ".kiss/rust_llvm_cov_cache"
    entry_paths = sorted((cache / "entries").glob("*.json"))
    assert entry_paths, f"missing Rust selector entries in {cache / 'entries'}"
    entry_payloads = [load_json(path) for path in entry_paths]
    assert_disjoint_entry_lines(entry_payloads, "src/lib.rs", 2, 6, 10)
    index = load_json(cache / "index.json")
    manifest = load_json(cache / "population.json")
    aggregate = load_json(cache / "check_aggregate.json")
    assert_index_source_selectors(index, "src/lib.rs", ("test_alpha", "test_beta"))
    aggregate_lines = {int(line) for line in aggregate["aggregate_covered_lines"]["src/lib.rs"]}
    assert {2, 6}.issubset(aggregate_lines), aggregate_lines
    assert 10 not in aggregate_lines, aggregate_lines
    assert_population_selectors(manifest, ("test_alpha", "test_beta"))
    artifact_paths = entry_paths + [
        cache / "index.json",
        cache / "population.json",
        cache / "check_aggregate.json",
    ]
    cold_bytes = relevant_artifact_bytes(artifact_paths)
    clear_markers(marker_dir)
    warm = run_witness_check("rust", repo, marker_dir, jobs=4)
    assert "refreshing Rust runtime coverage" not in warm.stderr, warm.stderr
    assert marker_names(marker_dir) == set()
    assert relevant_artifact_bytes(artifact_paths) == cold_bytes
    changed_text(repo / "src/lib.rs", "    \"alpha\"", "    { \"alpha\" }")
    dry = run_witness_dry_run("rust", repo, marker_dir)
    assert_dry_run_selects_exactly(dry, "test_alpha", "test_beta")
    assert metric_int(dry.metrics(), "selected_rust_initial") == 1
    assert "refreshing Rust runtime coverage" in cold.stderr, cold.stderr


def wait_for_barrier_ready(barrier_dir: Path, artifact: str, phase: str) -> dict:
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        for path in sorted(barrier_dir.glob("*.ready.json")):
            try:
                record = json.loads(path.read_text())
            except (OSError, json.JSONDecodeError):
                continue
            if record.get("artifact") == artifact and record.get("phase") == phase:
                return record
        time.sleep(0.02)
    raise AssertionError(f"timed out waiting for {artifact}:{phase} ready record")


def force_publication_target(repo: Path, language: str, artifact: str) -> None:
    if language == "python":
        cache = python_rslip_cache_root(repo)
        if artifact == "rslip_selector_entry":
            shutil.rmtree(cache / "entries", ignore_errors=True)
        elif artifact == "python_index":
            shutil.rmtree(cache / "entries", ignore_errors=True)
            (cache / "index.json").unlink(missing_ok=True)
        elif artifact == "python_population":
            shutil.rmtree(cache / "entries", ignore_errors=True)
            (cache / "index.json").unlink(missing_ok=True)
            (cache / "population.json").unlink(missing_ok=True)
        else:
            raise AssertionError(f"unknown Python publication artifact: {artifact}")
    else:
        cache = repo / ".kiss/rust_llvm_cov_cache"
        if artifact == "rust_selector_entry":
            shutil.rmtree(cache / "entries", ignore_errors=True)
            (cache / "check_aggregate.json").unlink(missing_ok=True)
            (cache / "index.json").unlink(missing_ok=True)
            (cache / "population.json").unlink(missing_ok=True)
        elif artifact == "rust_derived_index":
            (cache / "check_aggregate.json").unlink(missing_ok=True)
            (cache / "index.json").unlink(missing_ok=True)
        elif artifact == "rust_population":
            (cache / "check_aggregate.json").unlink(missing_ok=True)
            (cache / "index.json").unlink(missing_ok=True)
            (cache / "population.json").unlink(missing_ok=True)
        elif artifact == "rust_check_aggregate":
            (cache / "check_aggregate.json").unlink(missing_ok=True)
        else:
            raise AssertionError(f"unknown Rust publication artifact: {artifact}")


def assert_cache_json_integrity(repo: Path, language: str) -> None:
    cache = python_rslip_cache_root(repo) if language == "python" else repo / ".kiss/rust_llvm_cov_cache"
    assert_json_integrity(cache)


def run_publication_crash_scenario(
    root: Path,
    language: str,
    artifact: str,
    phase: str,
) -> None:
    slug = f"s{len(list(root.iterdir()))}"
    repo = root / f"{slug}r"
    markers = root / f"{slug}m"
    if language == "python":
        write_python_witness_repo(repo)
    else:
        write_rust_witness_repo(repo)
    baseline = run_witness_check(language, repo, markers)
    assert_check_gate_allowed(baseline)
    clear_markers(markers)
    force_publication_target(repo, language, artifact)

    barrier_dir = root / f"{slug}b"
    barrier_dir.mkdir()
    writer_env = witness_env(repo, markers)
    writer_env["KISS_QA_PUBLICATION_BARRIER_DIR"] = str(barrier_dir)
    writer_env["KISS_QA_PUBLICATION_BARRIER_TARGET"] = f"{artifact}:{phase}"
    writer_jobs = 1 if language == "rust" and artifact == "rust_selector_entry" else None
    writer = subprocess.Popen(
        witness_check_command(language, repo, jobs=writer_jobs),
        cwd=repo,
        env=writer_env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    ready = wait_for_barrier_ready(barrier_dir, artifact, phase)
    reader = subprocess.Popen(
        witness_check_command(language, repo),
        cwd=repo,
        env=witness_env(repo, markers),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    os.killpg(os.getpgid(writer.pid), signal.SIGKILL)
    writer_stdout, writer_stderr = writer.communicate(timeout=30)
    reader_stdout, reader_stderr = reader.communicate(timeout=300)
    writer_outcome = Outcome(
        f"{artifact}-{phase}-writer",
        writer.returncode,
        writer_stdout,
        writer_stderr,
        0.0,
    )
    reader_outcome = Outcome(
        f"{artifact}-{phase}-reader",
        reader.returncode,
        reader_stdout,
        reader_stderr,
        0.0,
    )
    click.echo(
        f"{artifact}:{phase}: writer_rc={writer_outcome.returncode} "
        f"reader_rc={reader_outcome.returncode}"
    )
    assert_check_gate_allowed(reader_outcome)
    staged = Path(ready["temporary_path"])
    if phase == "after_sync_before_rename":
        assert staged.exists(), f"expected staged temporary after pre-rename kill: {staged}"
        staged.unlink()
    assert_cache_json_integrity(repo, language)

    clear_markers(markers)
    recovery = run_concurrent(
        f"{artifact}-{phase}-recovery",
        [(witness_check_command(language, repo), repo) for _ in range(3)],
        witness_env(repo, markers),
        allow_failures=True,
    )
    for outcome in recovery:
        assert_check_gate_allowed(outcome)
    assert_cache_json_integrity(repo, language)
    clear_markers(markers)
    final_warm = run_witness_check(language, repo, markers)
    assert_check_gate_allowed(final_warm)
    refresh_message = (
        "refreshing Python runtime coverage"
        if language == "python"
        else "refreshing Rust runtime coverage"
    )
    assert refresh_message not in final_warm.stderr, final_warm.stderr
    assert marker_names(markers) == set()


def reset_rust_check_aggregate_outputs(repo: Path) -> None:
    rust_cache = repo / ".kiss/rust_llvm_cov_cache"
    (rust_cache / "check_aggregate.json").unlink(missing_ok=True)
    shutil.rmtree(rust_cache / "runs", ignore_errors=True)


def write_aggregate_benchmark_repo(repo: Path) -> None:
    (repo / "src").mkdir(parents=True)
    (repo / "tests").mkdir(parents=True)
    (repo / ".kissconfig").write_text(
        "[gate]\n"
        "test_coverage_threshold = 0\n"
        "duplication_enabled = false\n"
        "orphan_module_enabled = false\n",
    )
    (repo / "Cargo.toml").write_text(
        "[package]\n"
        "name = \"aggregate_benchmark\"\n"
        "version = \"0.1.0\"\n"
        "edition = \"2024\"\n",
    )
    (repo / "src/lib.rs").write_text("pub fn value() -> i32 { 1 }\n")
    (repo / "tests/slow.rs").write_text(
        "use std::fs;\n"
        "use std::path::PathBuf;\n"
        "use std::time::Duration;\n\n"
        "fn observe_active(name: &str) {\n"
        "    let root = PathBuf::from(std::env::var(\"KISS_AGG_BENCH_DIR\").unwrap());\n"
        "    let active = root.join(\"active\");\n"
        "    fs::create_dir_all(&active).unwrap();\n"
        "    let marker = active.join(format!(\"{}-{}\", std::process::id(), name));\n"
        "    fs::write(&marker, b\"1\").unwrap();\n"
        "    std::thread::sleep(Duration::from_millis(100));\n"
        "    let count = fs::read_dir(&active).unwrap().count();\n"
        "    let max_path = root.join(\"max_active\");\n"
        "    let previous = fs::read_to_string(&max_path)\n"
        "        .ok()\n"
        "        .and_then(|text| text.parse::<usize>().ok())\n"
        "        .unwrap_or(0);\n"
        "    if count > previous {\n"
        "        fs::write(&max_path, count.to_string()).unwrap();\n"
        "    }\n"
        "    std::thread::sleep(Duration::from_millis(900));\n"
        "    let _ = fs::remove_file(marker);\n"
        "}\n\n"
        "#[test]\nfn slow_a() { observe_active(\"a\"); assert_eq!(aggregate_benchmark::value(), 1); }\n"
        "#[test]\nfn slow_b() { observe_active(\"b\"); assert_eq!(aggregate_benchmark::value(), 1); }\n"
        "#[test]\nfn slow_c() { observe_active(\"c\"); assert_eq!(aggregate_benchmark::value(), 1); }\n"
        "#[test]\nfn slow_d() { observe_active(\"d\"); assert_eq!(aggregate_benchmark::value(), 1); }\n",
    )
    subprocess.run(
        ["cargo", "generate-lockfile"],
        cwd=repo,
        check=True,
        text=True,
        capture_output=True,
    )


def run_aggregate_benchmark_trial(
    repo: Path,
    env: dict[str, str],
    jobs: int,
    trial: int,
) -> Outcome:
    reset_rust_check_aggregate_outputs(repo)
    bench_dir = repo / ".kiss/aggregate_benchmark_active"
    shutil.rmtree(bench_dir, ignore_errors=True)
    bench_dir.mkdir(parents=True)
    trial_env = env.copy()
    trial_env["KISS_AGG_BENCH_DIR"] = str(bench_dir)
    outcome = run(
        f"rust-aggregate-benchmark-j{jobs}-{trial}",
        [
            str(KISS),
            "--defaults",
            "--lang",
            "rust",
            "cov",
            "-j",
            str(jobs),
            str(repo),
        ],
        repo,
        trial_env,
        expected=0,
    )
    counts = parse_rust_aggregate_refresh(outcome.stderr)
    assert counts is not None, outcome.stderr
    binaries, exports = counts
    assert binaries == 1 and exports == 1, outcome.stderr
    max_active = int((bench_dir / "max_active").read_text())
    if jobs > 1:
        assert max_active > 1, f"expected active test overlap for -j{jobs}, got {max_active}"
    return outcome


def assert_aggregate_parallel_benchmark() -> None:
    with tempfile.TemporaryDirectory(prefix="kiss-qa-rust-aggregate-bench-") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        write_aggregate_benchmark_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(ROOT)
        env.pop("RUSTFLAGS", None)
        warm = run_aggregate_benchmark_trial(repo, env, 4, 0)
        click.echo(f"rust-aggregate-benchmark-warmup elapsed={warm.elapsed:.2f}s")
        serial = [run_aggregate_benchmark_trial(repo, env, 1, i) for i in range(1, 4)]
        parallel = [run_aggregate_benchmark_trial(repo, env, 4, i) for i in range(1, 4)]
        serial_median = statistics.median(outcome.elapsed for outcome in serial)
        parallel_median = statistics.median(outcome.elapsed for outcome in parallel)
        click.echo(
            "Rust aggregate benchmark medians: "
            f"serial_j1={serial_median:.2f}s parallel_j4={parallel_median:.2f}s"
        )
        assert parallel_median < serial_median * 0.70, (
            f"parallel median {parallel_median:.2f}s is not < 70% of "
            f"serial median {serial_median:.2f}s"
        )


def python_rslip_cache_root(repo_root: Path) -> Path:
    machine_id = Path("/etc/machine-id").read_text().strip()
    assert machine_id, "Linux machine id must not be empty"
    host_component = machine_id.encode("ascii").hex()
    return repo_root / ".kiss" / "rslip_cache" / "hosts" / host_component


def assert_repo_relative_index(index: dict, expected_source: str) -> None:
    source_root = Path(index["source_root"])
    assert source_root.is_absolute(), source_root
    files = index["files"]
    assert expected_source in files, (
        f"changed source {expected_source!r} absent from index keys: "
        f"{sorted(files)[:20]}"
    )
    assert files, "coverage index unexpectedly empty"
    for file in files:
        pure = PurePosixPath(file)
        assert not pure.is_absolute(), file
        assert ".." not in pure.parts, file
        assert not file.startswith(".kiss/"), file
        assert not file.startswith("<"), file
        assert "rslip_runtime.py" not in file, file


def assert_json_integrity(cache_root: Path) -> int:
    json_paths = sorted(cache_root.rglob("*.json"))
    assert json_paths, f"no JSON artifacts under {cache_root}"
    for path in json_paths:
        try:
            json.loads(path.read_text())
        except Exception as error:
            raise AssertionError(f"invalid JSON artifact {path}: {error}") from error
    temporary = sorted(cache_root.rglob("*.tmp"))
    assert not temporary, f"temporary files survived: {temporary}"
    return len(json_paths)


def assert_no_transient_run_directories(rust_cache: Path) -> None:
    runs_root = rust_cache / "runs"
    if not runs_root.is_dir():
        return
    run_dirs = sorted(path for path in runs_root.iterdir() if path.is_dir())
    assert not run_dirs, f"transient run directories survived: {run_dirs[:3]}"


def assert_rust_observer_strictness(outcome: Outcome, jobs: int) -> None:
    observation = outcome.observation
    assert observation is not None, "observed run missing process observation"
    assert observation.llvm_single_thread_violations == 0, (
        "llvm-cov/llvm-profdata child missing single-thread flags: "
        f"{observation.llvm_single_thread_violations} violations"
    )
    assert observation.phase_overlap_samples == 0, (
        "build/test/export phases overlapped in "
        f"{observation.phase_overlap_samples} /proc samples"
    )
    assert observation.observed_build_jobs == jobs, (
        "cargo llvm-cov nextest --build-jobs mismatch: "
        f"expected {jobs}, observed {observation.observed_build_jobs}"
    )


def assert_rust_batch_invariants(outcome: Outcome, jobs: int) -> None:
    metrics = outcome.metrics()
    assert_metric(metrics, "rust_concurrency_budget", str(jobs))
    assert metric_int(metrics, "rust_build_target_count") <= 1
    assert metric_int(metrics, "rust_transient_residual_count") == 0
    assert_metric(metrics, "rust_external_tmp_residual_bytes", "0")
    assert_metric(metrics, "rust_external_tmp_residual_count", "0")
    active_tests = metric_int(metrics, "rust_max_active_test_instances")
    active_exports = metric_int(metrics, "rust_max_active_exports")
    assert active_tests <= jobs, (
        f"rust_max_active_test_instances={active_tests} exceeds jobs={jobs}"
    )
    assert active_exports <= jobs, (
        f"rust_max_active_exports={active_exports} exceeds jobs={jobs}"
    )
    assert metric_int(metrics, "rust_process_residual_count") == 0
    assert metric_int(metrics, "rust_entry_generation_count") <= 2
    max_objects = metric_int(metrics, "rust_max_objects_per_export")
    build_invocations = metric_int(metrics, "rust_build_invocations")
    if build_invocations > 0:
        assert max_objects > 0, (
            "fresh Rust batch should report per-export object scope"
        )


def echo_throughput_sample(sample: ThroughputSample) -> None:
    observation = sample.outcome.observation
    assert observation is not None
    peaks = ", ".join(
        f"{name}={count}" for name, count in sorted(observation.command_peaks.items())
    )
    click.echo(
        f"  {sample.phase} -j{sample.jobs}: elapsed={sample.outcome.elapsed:.2f}s "
        f"cache_bytes={sample.cache_bytes} peak_processes="
        f"{observation.peak_process_count} peak_threads="
        f"{observation.peak_thread_count} peak_rss_kib={observation.peak_rss_kib} "
        f"sampled_cpu_s={observation.sampled_cpu_seconds:.2f}"
    )
    if peaks:
        click.echo(f"    command_peaks: {peaks}")


def median_elapsed(samples: list[ThroughputSample], jobs: int, phase: str) -> float:
    values = [
        sample.outcome.elapsed
        for sample in samples
        if sample.jobs == jobs and sample.phase == phase
    ]
    assert values, f"missing {phase} samples for -j{jobs}"
    return statistics.median(values)


@click.group()
def cli() -> None:
    """Run long, disposable QA scenarios against target/debug/kiss."""


@cli.command("coverage-cache-witness")
def coverage_cache_witness() -> None:
    """Prove exact real coverage-cache payloads and warm non-execution."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-", dir="/tmp") as tmp:
        root = Path(tmp)
        py_repo = root / "p"
        rs_repo = root / "r"
        py_markers = root / "pm"
        rs_markers = root / "rm"
        write_python_witness_repo(py_repo)
        write_rust_witness_repo(rs_repo)
        assert_python_coverage_witness(py_repo, py_markers)
        assert_rust_coverage_witness(rs_repo, rs_markers)
        click.echo(
            "QA PASS: exact Python and Rust coverage witnesses, warm reuse, "
            "Python changed-line dry-run precision, and Rust aggregate-backed "
            "dry-run selection held."
        )


@cli.command("coverage-publication-crash-recovery")
def coverage_publication_crash_recovery() -> None:
    """Crash coverage publication at debug barriers and verify recovery."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    scenarios = [
        ("python", "rslip_selector_entry"),
        ("python", "python_index"),
        ("python", "python_population"),
        ("rust", "rust_selector_entry"),
        ("rust", "rust_derived_index"),
        ("rust", "rust_population"),
        ("rust", "rust_check_aggregate"),
    ]
    phases = ["after_sync_before_rename", "after_rename"]
    with tempfile.TemporaryDirectory(prefix="kq-crash-", dir="/tmp") as tmp:
        root = Path(tmp)
        for language, artifact in scenarios:
            for phase in phases:
                run_publication_crash_scenario(root, language, artifact, phase)
        click.echo(
            "QA PASS: barrier-targeted publication interruption and recovery "
            "held for Python and Rust check-published artifacts."
        )


@cli.command("coverage-stress")
def coverage_stress() -> None:
    """Stress population, selection, force, env invalidation, and recall."""
    jobs = 4
    with qa_fixture("kiss-qa-stress-") as fixture:
        cold: dict[str, Outcome] = {}
        warm: dict[str, Outcome] = {}
        forced: dict[str, Outcome] = {}

        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(jobs),
            )
            first = run(f"{language}-cold-dry-1", command, fixture.root, fixture.env)
            second = run(f"{language}-cold-dry-2", command, fixture.root, fixture.env)
            assert rendered_plan(first) == rendered_plan(second)
            assert f"{language.upper()} COVERAGE POPULATION" in first.stdout
            cold[language] = run(
                f"{language}-cold-population",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.root,
                fixture.env,
            )

        py_cold = cold["python"].metrics()
        rs_cold = cold["rust"].metrics()
        assert_metric(py_cold, "python_population_required", "true")
        assert_metric(rs_cold, "rust_population_required", "true")
        py_population = metric_int(py_cold, "python_population_selectors")
        rs_population = metric_int(rs_cold, "rust_population_selectors")
        assert py_population >= 4
        assert rs_population >= 8
        assert_metric(rs_cold, "raw_artifact_count", "0")
        assert_metric(rs_cold, "rust_external_tmp_residual_bytes", "0")
        assert_metric(rs_cold, "rust_external_tmp_residual_count", "0")
        assert metric_int(rs_cold, "rust_build_target_count") <= 1
        assert metric_int(rs_cold, "rust_transient_residual_count") == 0

        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(jobs),
            )
            first = run(f"{language}-warm-dry-1", command, fixture.root, fixture.env)
            second = run(f"{language}-warm-dry-2", command, fixture.root, fixture.env)
            assert rendered_plan(first) == rendered_plan(second)
            assert "COVERAGE POPULATION" not in first.stdout
            warm[language] = run(
                f"{language}-warm-selective",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.root,
                fixture.env,
            )
            forced[language] = run(
                f"{language}-forced-selective",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "--force",
                    "-j",
                    str(jobs),
                ),
                fixture.root,
                fixture.env,
            )
            post_force = run(
                f"{language}-post-force-dry",
                command,
                fixture.root,
                fixture.env,
            )
            assert "COVERAGE POPULATION" not in post_force.stdout, (
                f"{language}: a forced selective run invalidated the warm "
                "population manifest"
            )

        py_selected = metric_int(warm["python"].metrics(), "python_total")
        rs_selected = metric_int(warm["rust"].metrics(), "rust_final_total")
        assert 0 < py_selected <= py_population
        assert 0 < rs_selected <= rs_population
        assert metric_int(warm["python"].metrics(), "python_cache_hits") == py_selected
        assert metric_int(warm["rust"].metrics(), "rust_final_cache_hits") == rs_selected
        assert metric_int(forced["python"].metrics(), "python_cache_misses") == py_selected
        assert (
            metric_int(forced["rust"].metrics(), "rust_final_cache_misses") == rs_selected
        )

        changed_py_env = fixture.env.copy()
        changed_py_env["PYTHONPATH"] = (
            f"{fixture.root}{os.pathsep}/tmp/kiss-qa-env-change"
        )
        py_env = run(
            "python-env-invalidation",
            kiss_command(
                "python",
                fixture.ignores["python"],
                "--dry-run",
                "-j",
                str(jobs),
            ),
            fixture.root,
            changed_py_env,
        )
        assert "PYTHON COVERAGE POPULATION" in py_env.stdout

        changed_rs_env = fixture.env.copy()
        changed_rs_env["RUSTFLAGS"] = "-Cdebuginfo=0"
        rs_env = run(
            "rust-env-invalidation",
            kiss_command(
                "rust",
                fixture.ignores["rust"],
                "--dry-run",
                "-j",
                str(jobs),
            ),
            fixture.root,
            changed_rs_env,
        )
        assert "RUST COVERAGE POPULATION" in rs_env.stdout

        for language in LANGUAGES:
            validation = run(
                f"{language}-validation",
                validate_command(language, fixture.ignores[language], jobs),
                fixture.root,
                fixture.env,
            )
            metrics = validation.metrics()
            assert 0 < metric_int(metrics, "selected_total") <= metric_int(
                metrics, "full_total"
            )

        changed_text(
            fixture.root / PY_SOURCE,
            "return {path: partial.get(path, float(0)) for path in files}",
            "return {path: partial.get(path, 999.0) for path in files}",
        )
        py_regression = run(
            "python-regression-kiss",
            kiss_command(
                "python",
                fixture.ignores["python"],
                "--metrics",
                "-j",
                str(jobs),
            ),
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert py_regression.returncode != 0
        py_oracle = run(
            "python-regression-oracle",
            ["python", "-m", "pytest", str(PY_TEST), "-q"],
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert py_oracle.returncode != 0

        changed_text(
            fixture.root / RS_SOURCE,
            "if 100 <= file_pct {",
            "if 1000 <= file_pct {",
        )
        rs_regression = run(
            "rust-regression-kiss",
            kiss_command(
                "rust",
                fixture.ignores["rust"],
                "--metrics",
                "-j",
                str(jobs),
            ),
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert rs_regression.returncode != 0
        rs_oracle = run(
            "rust-regression-oracle",
            [
                "cargo",
                "test",
                "test_format_unreferenced_unit_coverage_message_rounding_cliff",
            ],
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert rs_oracle.returncode != 0
        click.echo("QA PASS: coverage lifecycle and oracle recall held.")


@cli.command("rust-throughput")
@click.option("--runs", default=3, show_default=True, help="Samples per job value.")
@click.option(
    "--jobs",
    "job_values",
    multiple=True,
    type=int,
    default=(1, 2, 4, 32),
    show_default=True,
    help="KISS -j values to measure.",
)
@click.option(
    "--legacy-cold-j1-median",
    type=float,
    default=None,
    help="Optional legacy cold -j1 median seconds for acceptance comparison.",
)
def rust_throughput(
    runs: int,
    job_values: tuple[int, ...],
    legacy_cold_j1_median: float | None,
) -> None:
    """Measure Rust coverage throughput and external process-tree bounds."""
    assert runs > 0, runs
    assert job_values, "at least one --jobs value is required"
    assert all(jobs > 0 for jobs in job_values), job_values
    samples: list[ThroughputSample] = []
    with qa_fixture("kiss-qa-rust-throughput-") as fixture:
        rust_cache = fixture.root / ".kiss/rust_llvm_cov_cache"
        for sample_index in range(runs):
            for jobs in job_values:
                shutil.rmtree(rust_cache, ignore_errors=True)
                command = kiss_command(
                    "rust",
                    fixture.ignores["rust"],
                    "--metrics",
                    "-j",
                    str(jobs),
                )
                cold = run_observed(
                    f"rust-throughput-cold-{sample_index + 1}-j{jobs}",
                    command,
                    fixture.root,
                    fixture.env,
                )
                assert_rust_batch_invariants(cold, jobs)
                cold_sample = ThroughputSample(
                    jobs,
                    "cold",
                    cold,
                    directory_size_bytes(rust_cache),
                )
                samples.append(cold_sample)
                echo_throughput_sample(cold_sample)

                warm = run_observed(
                    f"rust-throughput-warm-{sample_index + 1}-j{jobs}",
                    command,
                    fixture.root,
                    fixture.env,
                )
                assert_rust_batch_invariants(warm, jobs)
                warm_sample = ThroughputSample(
                    jobs,
                    "warm",
                    warm,
                    directory_size_bytes(rust_cache),
                )
                samples.append(warm_sample)
                echo_throughput_sample(warm_sample)

    click.echo("Rust throughput medians:")
    for jobs in job_values:
        cold_median = median_elapsed(samples, jobs, "cold")
        warm_median = median_elapsed(samples, jobs, "warm")
        click.echo(f"  -j{jobs}: cold_median={cold_median:.2f}s warm_median={warm_median:.2f}s")

    if legacy_cold_j1_median is not None and 32 in job_values:
        batch_j32 = median_elapsed(samples, 32, "cold")
        required = legacy_cold_j1_median * 0.70
        assert batch_j32 <= required, (
            f"batch cold -j32 median {batch_j32:.2f}s is not at least 30% faster "
            f"than legacy cold -j1 median {legacy_cold_j1_median:.2f}s"
        )
        click.echo(
            "QA PASS: Rust throughput median met the legacy cold -j1 acceptance threshold."
        )
    else:
        click.echo(
            "QA PASS: Rust throughput medians and external process-tree bounds recorded."
        )


@cli.command("path-isolation")
def path_isolation() -> None:
    """Test nested-CWD plans and persisted coverage-path isolation."""
    jobs = 2
    with qa_fixture("kiss-qa-paths-") as fixture:
        cold_metrics: dict[str, dict[str, str]] = {}
        warm_metrics: dict[str, dict[str, str]] = {}
        for language in LANGUAGES:
            dry_command = kiss_command(
                language,
                fixture.ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(jobs),
            )
            root_dry = run(
                f"{language}-root-dry", dry_command, fixture.root, fixture.env
            )
            nested_dry = run(
                f"{language}-nested-dry", dry_command, fixture.nested, fixture.env
            )
            assert rendered_plan(root_dry) == rendered_plan(nested_dry)
            assert f"{language.upper()} COVERAGE POPULATION" in root_dry.stdout
            cold = run(
                f"{language}-nested-population",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.nested,
                fixture.env,
            )
            cold_metrics[language] = cold.metrics()

        py_cache = python_rslip_cache_root(fixture.root)
        rs_cache = fixture.root / ".kiss/rust_llvm_cov_cache"
        py_index = load_json(py_cache / "index.json")
        rs_index = load_json(rs_cache / "index.json")
        py_manifest = load_json(py_cache / "population.json")
        rs_manifest = load_json(rs_cache / "population.json")
        for payload in (py_index, rs_index, py_manifest, rs_manifest):
            assert Path(payload["source_root"]).resolve() == fixture.root.resolve()
        assert_repo_relative_index(py_index, PY_SOURCE.as_posix())
        assert_repo_relative_index(rs_index, RS_SOURCE.as_posix())

        py_population = metric_int(
            cold_metrics["python"], "python_population_selectors"
        )
        rs_population = metric_int(cold_metrics["rust"], "rust_population_selectors")
        assert len(py_manifest["selectors"]) == py_population
        assert len(set(py_manifest["selectors"])) == py_population
        assert 0 < len(rs_manifest["selectors"]) <= rs_population
        assert len(set(rs_manifest["selectors"])) == len(rs_manifest["selectors"])
        assert_metric(cold_metrics["rust"], "raw_artifact_count", "0")
        assert_metric(cold_metrics["rust"], "rust_external_tmp_residual_bytes", "0")
        assert_metric(cold_metrics["rust"], "rust_external_tmp_residual_count", "0")
        assert metric_int(cold_metrics["rust"], "rust_build_target_count") <= 1
        assert metric_int(cold_metrics["rust"], "rust_transient_residual_count") == 0

        for language in LANGUAGES:
            warm = run(
                f"{language}-nested-warm",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.nested,
                fixture.env,
            )
            warm_metrics[language] = warm.metrics()
        py_selected = metric_int(warm_metrics["python"], "python_total")
        rs_selected = metric_int(warm_metrics["rust"], "rust_final_total")
        assert 0 < py_selected <= py_population
        assert 0 < rs_selected <= rs_population
        assert metric_int(warm_metrics["python"], "python_cache_hits") == py_selected
        assert (
            metric_int(warm_metrics["rust"], "rust_final_cache_hits") == rs_selected
        )
        click.echo(
            "QA PASS: CWD-stable plans, clean persisted paths, manifests, "
            "worker bounds, and warm selection held."
        )


@cli.command("concurrent-cache-recovery")
def concurrent_cache_recovery() -> None:
    """Race shared caches, then test malformed-index fail-safe recovery."""
    jobs = 2
    with qa_fixture("kiss-qa-concurrent-") as fixture:
        cold_commands: list[tuple[list[str], Path]] = []
        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--metrics",
                "-j",
                str(jobs),
            )
            cold_commands.extend(
                [
                    (command, fixture.root),
                    (command, fixture.nested),
                    (command, fixture.root),
                ]
            )
        cold = run_concurrent("cold-race", cold_commands, fixture.env)
        for outcome in cold[:3]:
            assert outcome.metrics()["python_population_required"] == "true"
        for outcome in cold[3:]:
            assert outcome.metrics()["rust_population_required"] == "true"
        rust_cold_metrics = [outcome.metrics() for outcome in cold[3:]]
        rust_universes = {
            metric_int(metrics, "rust_population_selectors") for metrics in rust_cold_metrics
        }
        assert len(rust_universes) == 1, rust_universes
        rust_universe = rust_universes.pop()
        rust_cold_summary = [
            {
                "name": outcome.name,
                "total": metric_int(metrics, "rust_population_total"),
                "hits": metric_int(metrics, "rust_population_cache_hits"),
                "misses": metric_int(metrics, "rust_population_cache_misses"),
                "final_total": metric_int(metrics, "rust_final_total"),
                "final_hits": metric_int(metrics, "rust_final_cache_hits"),
                "final_misses": metric_int(metrics, "rust_final_cache_misses"),
            }
            for outcome, metrics in zip(cold[3:], rust_cold_metrics)
        ]
        rust_total_misses = sum(
            metric_int(metrics, "rust_population_cache_misses")
            for metrics in rust_cold_metrics
        )
        rust_total_hits = sum(
            metric_int(metrics, "rust_population_cache_hits") for metrics in rust_cold_metrics
        )
        assert rust_total_hits + rust_total_misses == len(rust_cold_metrics) * rust_universe, (
            rust_cold_summary
        )
        assert rust_universe <= rust_total_misses <= len(rust_cold_metrics) * rust_universe, (
            rust_cold_summary
        )
        click.echo(
            "rust cold race accounting: "
            f"universe={rust_universe} hits={rust_total_hits} misses={rust_total_misses}"
        )

        py_cache = python_rslip_cache_root(fixture.root)
        rs_cache = fixture.root / ".kiss/rust_llvm_cov_cache"
        py_json_count = assert_json_integrity(py_cache)
        rs_json_count = assert_json_integrity(rs_cache)
        assert not list((rs_cache / "artifacts").glob("*"))
        workers = [
            path
            for path in (rs_cache / "workers").glob("slot-*")
            if path.is_dir()
        ]
        assert len(workers) == 0, (
            f"legacy worker slots must be absent after batch migration: {workers}"
        )
        for outcome in cold[3:]:
            metrics = outcome.metrics()
            assert_metric(metrics, "rust_external_tmp_residual_bytes", "0")
            assert_metric(metrics, "rust_external_tmp_residual_count", "0")
            assert metric_int(metrics, "rust_build_target_count") <= 1
        click.echo(
            f"cold integrity: python_json={py_json_count} "
            f"rust_json={rs_json_count} workers={len(workers)}"
        )

        dry_commands: list[tuple[list[str], Path]] = []
        dry_languages: list[str] = []
        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(jobs),
            )
            for cwd in (
                fixture.root,
                fixture.nested,
                fixture.root,
                fixture.nested,
            ):
                dry_commands.append((command, cwd))
                dry_languages.append(language)
        dry = run_concurrent("warm-dry-race", dry_commands, fixture.env)
        for language in LANGUAGES:
            plans = [
                rendered_plan(outcome)
                for outcome, outcome_language in zip(
                    dry, dry_languages, strict=True
                )
                if outcome_language == language
            ]
            assert len(set(plans)) == 1
            assert "COVERAGE POPULATION" not in plans[0]

        for language in LANGUAGES:
            run(
                f"{language}-warm-prime",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.root,
                fixture.env,
            )

        warm_commands: list[tuple[list[str], Path]] = []
        warm_languages: list[str] = []
        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--metrics",
                "-j",
                str(jobs),
            )
            for cwd in (fixture.root, fixture.nested):
                warm_commands.append((command, cwd))
                warm_languages.append(language)
        warm = run_concurrent("warm-execution-race", warm_commands, fixture.env)
        for outcome, language in zip(warm, warm_languages, strict=True):
            metrics = outcome.metrics()
            if language == "python":
                total = metric_int(metrics, "python_total")
                hits = metric_int(metrics, "python_cache_hits")
            else:
                total = metric_int(metrics, "rust_final_total")
                hits = metric_int(metrics, "rust_final_cache_hits")
            assert total > 0
            missed_lines = [
                line
                for line in outcome.stdout.splitlines()
                if line.startswith("PASSED:") or line.startswith("FAILED:")
            ]
            assert hits == total, (
                f"{outcome.name} {language}: expected all warm selectors to be "
                f"cache hits, got hits={hits}, total={total}, metrics={metrics}, "
                f"missed_lines={missed_lines}"
            )
            if language == "rust":
                assert_metric(metrics, "rust_external_tmp_residual_bytes", "0")
                assert_metric(metrics, "rust_external_tmp_residual_count", "0")
                assert_metric(metrics, "rust_external_tmp_residuals_pass", "true")

        for language, index_path in (
            ("python", py_cache / "index.json"),
            ("rust", rs_cache / "index.json"),
        ):
            index_path.write_text("{ deliberately broken")
            corrupted_dry = run(
                f"{language}-corrupt-index-dry",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--dry-run",
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.nested,
                fixture.env,
            )
            assert f"{language.upper()} COVERAGE POPULATION" in corrupted_dry.stdout
            repaired = run(
                f"{language}-corrupt-index-repair",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.nested,
                fixture.env,
            )
            assert repaired.metrics()[f"{language}_population_required"] == "true"
            json.loads(index_path.read_text())

        assert_json_integrity(py_cache)
        assert_json_integrity(rs_cache)
        assert not list((rs_cache / "artifacts").glob("*"))
        click.echo(
            "QA PASS: concurrent cold/warm races and malformed-index recovery held."
        )


@cli.command("rust-batch-e2e")
def rust_batch_e2e() -> None:
    """E2E batch QA: nocapture relay, forced serialization, derived repair, Ctrl-C."""
    jobs = 2
    with qa_fixture("kiss-qa-rust-e2e-") as fixture:
        rust_cache = fixture.root / ".kiss/rust_llvm_cov_cache"
        population_command = kiss_command(
            "rust",
            fixture.ignores["rust"],
            "--metrics",
            "-j",
            str(jobs),
        )

        cold = run(
            "rust-e2e-cold-population",
            population_command,
            fixture.root,
            fixture.env,
        )
        assert_metric(cold.metrics(), "rust_population_required", "true")
        assert_json_integrity(rust_cache)

        nocapture_dry = run(
            "rust-e2e-nocapture-dry",
            kiss_command(
                "rust",
                fixture.ignores["rust"],
                "--dry-run",
                "-j",
                str(jobs),
                trailing_test_args=("--nocapture",),
            ),
            fixture.root,
            fixture.env,
        )
        assert "'--test-threads' 1" in nocapture_dry.stdout, (
            "nocapture must plan serial nextest test threads"
        )

        nocapture = run_observed(
            "rust-e2e-nocapture",
            kiss_command(
                "rust",
                fixture.ignores["rust"],
                "--metrics",
                "--force",
                "-j",
                str(jobs),
                trailing_test_args=("--nocapture",),
            ),
            fixture.root,
            fixture.env,
        )
        nocapture_metrics = nocapture.metrics()
        assert nocapture.returncode == 0, nocapture.combined
        assert "KISS TEST METRICS" in nocapture.stdout
        assert_rust_batch_invariants(nocapture, jobs)
        assert metric_int(nocapture_metrics, "rust_population_cache_misses") > 0

        forced_commands = [
            (
                kiss_command(
                    "rust",
                    fixture.ignores["rust"],
                    "--metrics",
                    "--force",
                    "-j",
                    str(jobs),
                ),
                fixture.root,
            ),
            (
                kiss_command(
                    "rust",
                    fixture.ignores["rust"],
                    "--metrics",
                    "--force",
                    "-j",
                    str(jobs),
                ),
                fixture.root,
            ),
        ]
        forced = run_concurrent("rust-e2e-concurrent-forced", forced_commands, fixture.env)
        for outcome in forced:
            metrics = outcome.metrics()
            population = metric_int(metrics, "rust_population_selectors")
            misses = metric_int(metrics, "rust_population_cache_misses")
            assert misses == population, (
                f"{outcome.name}: forced fresh batch should miss every selector, "
                f"misses={misses}, population={population}"
            )
            assert_rust_batch_invariants(outcome, jobs)

        population_path = rust_cache / "population.json"
        population_path.write_text("{ deliberately broken")
        repaired = run(
            "rust-e2e-derived-repair-population",
            population_command,
            fixture.root,
            fixture.env,
        )
        repair_metrics = repaired.metrics()
        if repair_metrics.get("rust_derived_repair") == "true":
            assert metric_int(repair_metrics, "rust_build_invocations") == 0
            assert metric_int(repair_metrics, "rust_population_cache_misses") == 0
        else:
            assert (
                metric_int(repair_metrics, "rust_population_cache_misses")
                == metric_int(repair_metrics, "rust_population_selectors")
            )
            assert metric_int(repair_metrics, "rust_build_invocations") > 0
        json.loads(population_path.read_text())

        interrupted = run_interrupted(
            "rust-e2e-interrupt",
            kiss_command(
                "rust",
                fixture.ignores["rust"],
                "--metrics",
                "--force",
                "-j",
                str(jobs),
            ),
            fixture.root,
            fixture.env,
            signal_after=0.75,
        )
        assert interrupted.returncode != 0, "interrupted batch should fail"
        residual = lingering_processes_matching(
            (str(fixture.root), "rust_llvm_cov_cache")
        )
        assert not residual, f"batch descendants survived interruption: {residual}"
        assert_no_transient_run_directories(rust_cache)
        recovered = run(
            "rust-e2e-recover-after-interrupt",
            population_command,
            fixture.root,
            fixture.env,
        )
        assert recovered.returncode == 0, recovered.combined
        assert metric_int(recovered.metrics(), "rust_process_residual_count") == 0
        assert_json_integrity(rust_cache)

        forced_population_command = kiss_command(
            "rust",
            fixture.ignores["rust"],
            "--metrics",
            "--force",
            "-j",
            str(jobs),
        )
        signal_ignoring = run_interrupted(
            "rust-e2e-interrupt-signal-ignoring",
            [
                "/bin/sh",
                "-c",
                (
                    "trap '' INT; "
                    + " ".join(shlex_quote(part) for part in forced_population_command)
                ),
            ],
            fixture.root,
            fixture.env,
            signal_after=1.5,
        )
        assert signal_ignoring.returncode is not None
        residual = lingering_processes_matching((str(fixture.root), "sleep"))
        assert not residual, f"signal-ignoring descendants survived: {residual}"
        recovered_after_signal = run(
            "rust-e2e-recover-after-signal-ignoring",
            population_command,
            fixture.root,
            fixture.env,
        )
        assert recovered_after_signal.returncode == 0, recovered_after_signal.combined
        assert_json_integrity(rust_cache)
        click.echo(
            "QA PASS: nocapture relay, concurrent forced batches, derived repair, "
            "Ctrl-C recovery, and signal-ignoring cleanup held."
        )


@cli.command("aggregate-coverage")
def aggregate_coverage() -> None:
    """QA for Rust check aggregate publication, warm reuse, and repair."""
    jobs = 4
    with qa_fixture("kiss-qa-rust-aggregate-") as fixture:
        command = [
            str(KISS),
            "--defaults",
            "--lang",
            "rust",
            "cov",
            "-j",
            str(jobs),
            *fixture.ignores["rust"],
            str(fixture.root),
        ]
        cold = run(
            "rust-aggregate-cold-check",
            command,
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert "GATE_FAILED:test_coverage" in cold.stdout or cold.returncode == 0, cold.combined
        cold_counts = parse_rust_aggregate_refresh(cold.stderr)
        assert cold_counts is not None, cold.stderr
        cold_binaries, cold_exports = cold_counts
        assert cold_binaries > 0, cold.stderr
        assert 0 < cold_exports <= cold_binaries, cold.stderr
        aggregate = fixture.root / ".kiss/rust_llvm_cov_cache/check_aggregate.json"
        data = load_json(aggregate)
        assert data["schema_version"] == "rust-check-aggregate-v1"
        assert data["binaries"], "aggregate must contain at least one binary record"
        selector_count = len(data["selector_binary_ids"])
        if selector_count > 1:
            assert cold_exports < selector_count, (
                "aggregate should export per binary, not per selected test instance: "
                f"selectors={selector_count} exports={cold_exports}"
            )
        cold_maps = {
            record["id"]: record["line_map"]
            for record in data["binaries"]
        }

        warm = run(
            "rust-aggregate-warm-check",
            command,
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert "GATE_FAILED:test_coverage" in warm.stdout or warm.returncode == 0, warm.combined
        assert "refreshing Rust runtime coverage" not in warm.stderr, warm.stderr

        source = fixture.root / RS_SOURCE
        source.write_text(source.read_text() + "\n// aggregate coverage identity repair\n")
        identity = run(
            "rust-aggregate-identity-repair",
            command,
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert "GATE_FAILED:test_coverage" in identity.stdout or identity.returncode == 0, (
            identity.combined
        )
        identity_counts = parse_rust_aggregate_refresh(identity.stderr)
        assert identity_counts is not None, identity.stderr
        identity_binaries, identity_exports = identity_counts
        assert identity_binaries == cold_binaries, identity.stderr
        assert identity_exports == 0, identity.stderr

        changed_text(source, "if 100 <= file_pct {", "if file_pct >= 100 {")
        repair = run(
            "rust-aggregate-code-repair",
            command,
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert "GATE_FAILED:test_coverage" in repair.stdout or repair.returncode == 0, (
            repair.combined
        )
        repair_counts = parse_rust_aggregate_refresh(repair.stderr)
        assert repair_counts is not None, repair.stderr
        repair_binaries, repair_exports = repair_counts
        assert repair_binaries == cold_binaries, repair.stderr
        assert 0 < repair_exports <= repair_binaries, repair.stderr
        repaired = load_json(aggregate)
        repaired_maps = {
            record["id"]: record["line_map"]
            for record in repaired["binaries"]
        }
        assert set(repaired_maps) == set(cold_maps), "repair changed aggregate binary set"
        retained_maps = sum(
            1 for binary_id, line_map in cold_maps.items()
            if repaired_maps.get(binary_id) == line_map
        )
        assert retained_maps >= cold_binaries - repair_exports, (
            "repair should retain unchanged binary maps exactly: "
            f"retained={retained_maps} binaries={cold_binaries} exports={repair_exports}"
        )

        final_warm = run(
            "rust-aggregate-final-warm-check",
            command,
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert "GATE_FAILED:test_coverage" in final_warm.stdout or final_warm.returncode == 0, (
            final_warm.combined
        )
        assert "refreshing Rust runtime coverage" not in final_warm.stderr, final_warm.stderr
        aggregate.unlink()
        shutil.rmtree(fixture.root / ".kiss/rust_llvm_cov_cache/runs", ignore_errors=True)
        concurrent = run_concurrent(
            "rust-aggregate-concurrent-check",
            [(command, fixture.root), (command, fixture.root)],
            fixture.env,
            allow_failures=True,
        )
        for outcome in concurrent:
            assert_check_gate_allowed(outcome)
        refreshers = sum(
            "refreshing Rust runtime coverage" in outcome.stderr
            for outcome in concurrent
        )
        assert refreshers == 1, (
            "concurrent refresh should have exactly one writer and one waiter/reloader: "
            f"{[outcome.stderr for outcome in concurrent]}"
        )
        assert aggregate.is_file(), "concurrent refresh should leave a published aggregate"
        concurrent_data = load_json(aggregate)
        assert concurrent_data["binaries"], "concurrent refresh published an empty aggregate"
        post_concurrent = run(
            "rust-aggregate-post-concurrent-warm-check",
            command,
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert_check_gate_allowed(post_concurrent)
        assert "refreshing Rust runtime coverage" not in post_concurrent.stderr, (
            post_concurrent.stderr
        )
        assert_aggregate_parallel_benchmark()
        click.echo(
            "QA PASS: Rust check aggregate cold publication, warm reuse, "
            "identity repair, code repair, retained maps, concurrent refresh, "
            "and serial/parallel benchmark held."
        )


@cli.command("rust-phase-interrupt")
def rust_phase_interrupt() -> None:
    """Interrupt compile-once Rust coverage separately during build, test, and export."""
    jobs = 2
    log_dir = (
        Path.home()
        / ".malvin_home"
        / "logs"
        / "d5af67e712b1a200"
        / "20260712_013825_efle49i0"
    )
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / "phase_interrupt.log"
    phase_delays = {"build": 0.35, "test": 12.0, "export": 8.0}
    with qa_fixture("kiss-qa-rust-phase-interrupt-") as fixture, log_path.open("w") as log:
        population_command = kiss_command(
            "rust",
            fixture.ignores["rust"],
            "--metrics",
            "--force",
            "-j",
            str(jobs),
        )
        for phase, signal_after in phase_delays.items():
            interrupted = run_interrupted(
                f"rust-phase-interrupt-{phase}",
                population_command,
                fixture.root,
                fixture.env,
                signal_after=signal_after,
            )
            log.write(
                f"{phase}: rc={interrupted.returncode} elapsed={interrupted.elapsed:.2f}s "
                f"signal_after={signal_after}\n"
            )
            assert interrupted.returncode != 0, f"{phase} interrupt should fail"
            residual = lingering_processes_matching(
                (str(fixture.root), "rust_llvm_cov_cache")
            )
            assert not residual, f"{phase} descendants survived: {residual}"
            assert_no_transient_run_directories(fixture.root / ".kiss/rust_llvm_cov_cache")
            recovered = run(
                f"rust-phase-recover-{phase}",
                population_command,
                fixture.root,
                fixture.env,
            )
            assert recovered.returncode == 0, recovered.combined
            log.write(f"{phase}-recover: rc={recovered.returncode}\n")
        click.echo(f"QA PASS: phase-specific Ctrl-C recovery held. Log: {log_path}")


@cli.command("rust-legacy-warm-baseline")
@click.option(
    "--batch-warm-median",
    type=float,
    default=3.86,
    show_default=True,
    help="Batch warm all-hit median seconds for <=10% regression check.",
)
@click.option(
    "--log-dir",
    type=click.Path(path_type=Path),
    default=None,
    help="Directory for archived baseline logs.",
)
def rust_legacy_warm_baseline(batch_warm_median: float, log_dir: Path | None) -> None:
    """Verify batch warm all-hit median against archived legacy baseline."""
    archive_dir = log_dir or (
        Path.home()
        / ".malvin_home"
        / "logs"
        / "d5af67e712b1a200"
        / "20260711_175351_ua0kwyo6"
    )
    archive_dir.mkdir(parents=True, exist_ok=True)
    log_path = archive_dir / "legacy_warm_baseline.log"
    assert log_path.is_file(), (
        f"missing archived legacy warm baseline at {log_path}; "
        "record it before removing the legacy backend"
    )
    legacy_median = None
    for line in log_path.read_text().splitlines():
        if line.startswith("legacy_warm_median_s="):
            legacy_median = float(line.split("=", 1)[1].split()[0])
    assert legacy_median is not None, (
        "archived legacy warm baseline did not contain legacy_warm_median_s="
    )
    allowed = legacy_median * 1.10
    assert batch_warm_median <= allowed, (
        f"batch warm median {batch_warm_median:.2f}s regressed more than 10% "
        f"vs legacy warm median {legacy_median:.2f}s (allowed {allowed:.2f}s)"
    )
    click.echo(f"Using archived legacy warm baseline: {log_path}")
    click.echo(
        f"QA PASS: batch warm median {batch_warm_median:.2f}s within 10% of "
        f"legacy warm median {legacy_median:.2f}s."
    )


@cli.command("rust-full-repo-observer")
@click.option("--jobs", default=32, show_default=True, help="KISS -j value for cold population.")
@click.option(
    "--log-dir",
    type=click.Path(path_type=Path),
    default=None,
    help="Directory for archived observer logs.",
)
def rust_full_repo_observer(jobs: int, log_dir: Path | None) -> None:
    """Observe full-repository cold Rust population process/thread bounds."""
    archive_dir = log_dir or (
        Path.home()
        / ".malvin_home"
        / "logs"
        / "d5af67e712b1a200"
        / "20260711_175351_ua0kwyo6"
    )
    archive_dir.mkdir(parents=True, exist_ok=True)
    release_kiss = ROOT / "target" / "release" / "kiss"
    kiss_bin = release_kiss if release_kiss.is_file() else KISS
    env = os.environ.copy()
    env["PYTHONPATH"] = str(ROOT)
    rust_cache = ROOT / ".kiss" / "rust_llvm_cov_cache"
    shutil.rmtree(rust_cache, ignore_errors=True)
    command = [
        str(kiss_bin),
        "--defaults",
        "--lang",
        "rust",
        "test",
        "commit",
        "--metrics",
        "-j",
        str(jobs),
    ]
    outcome = run_observed(
        "rust-full-repo-observer-cold",
        command,
        ROOT,
        env,
        timeout=2_400,
    )
    assert_rust_batch_invariants(outcome, jobs)
    assert_rust_observer_strictness(outcome, jobs)
    metrics = outcome.metrics()
    observation = outcome.observation
    assert observation is not None
    active_tests = metric_int(metrics, "rust_max_active_test_instances")
    active_exports = metric_int(metrics, "rust_max_active_exports")
    assert active_tests <= jobs
    assert active_exports <= jobs
    log_path = archive_dir / f"full_repo_observer_j{jobs}.log"
    peaks = ", ".join(
        f"{name}={count}"
        for name, count in sorted(observation.command_peaks.items())
    )
    log_path.write_text(
        "\n".join(
            [
                f"elapsed={outcome.elapsed:.2f}",
                f"peak_processes={observation.peak_process_count}",
                f"peak_threads={observation.peak_thread_count}",
                f"peak_rss_kib={observation.peak_rss_kib}",
                f"sampled_cpu_s={observation.sampled_cpu_seconds:.2f}",
                f"command_peaks={peaks}",
                f"rust_max_active_test_instances={active_tests}",
                f"rust_max_active_exports={active_exports}",
                f"phase_rust_export_ms={metrics.get('phase_rust_export_ms', 'missing')}",
                f"phase_overlap_samples={observation.phase_overlap_samples}",
                f"llvm_single_thread_violations={observation.llvm_single_thread_violations}",
                f"observed_build_jobs={observation.observed_build_jobs}",
                "",
                outcome.stdout,
                outcome.stderr,
            ]
        )
    )
    click.echo(f"Archived observer evidence: {log_path}")
    click.echo(
        "QA PASS: full-repository external observer recorded process/thread bounds, "
        "build-jobs token count, phase non-overlap, and single-thread LLVM argv."
    )


@cli.command("rust-retained-cache-audit")
@click.option(
    "--log-dir",
    type=click.Path(path_type=Path),
    default=None,
    help="Directory for archived retained-cache audit logs.",
)
def rust_retained_cache_audit(log_dir: Path | None) -> None:
    """Audit retained Rust cache bounds across jobs and repeated generations."""
    archive_dir = log_dir or (
        Path.home()
        / ".malvin_home"
        / "logs"
        / "d5af67e712b1a200"
        / "20260712_013825_efle49i0"
    )
    archive_dir.mkdir(parents=True, exist_ok=True)
    jobs_values = (1, 4)
    with qa_fixture("kiss-qa-retained-cache-") as fixture:
        rust_cache = fixture.root / ".kiss/rust_llvm_cov_cache"
        population_command = kiss_command(
            "rust",
            fixture.ignores["rust"],
            "--metrics",
            "--force",
        )
        cache_bytes_by_jobs: dict[int, int] = {}
        lines: list[str] = []
        for jobs in jobs_values:
            outcome = run(
                f"rust-retained-cache-j{jobs}",
                population_command + ["-j", str(jobs)],
                fixture.root,
                fixture.env,
            )
            assert outcome.returncode == 0, outcome.combined
            metrics = outcome.metrics()
            assert_rust_batch_invariants(outcome, jobs)
            cache_bytes_by_jobs[jobs] = metric_int(metrics, "rust_entry_cache_bytes")
            lines.append(
                f"jobs={jobs} rust_entry_cache_bytes={cache_bytes_by_jobs[jobs]} "
                f"rust_entry_generation_count="
                f"{metric_int(metrics, 'rust_entry_generation_count')}"
            )
        assert cache_bytes_by_jobs[1] == cache_bytes_by_jobs[4], (
            f"cache bytes grew with jobs: {cache_bytes_by_jobs}"
        )
        second = run(
            "rust-retained-cache-second-generation",
            population_command + ["-j", "1"],
            fixture.root,
            fixture.env,
        )
        assert second.returncode == 0, second.combined
        second_metrics = second.metrics()
        assert metric_int(second_metrics, "rust_entry_generation_count") <= 2
        build_target_bytes = metric_int(second_metrics, "rust_build_target_bytes")
        third = run(
            "rust-retained-cache-third-generation",
            population_command + ["-j", "1"],
            fixture.root,
            fixture.env,
        )
        assert third.returncode == 0, third.combined
        third_metrics = third.metrics()
        third_build_target_bytes = metric_int(third_metrics, "rust_build_target_bytes")
        assert third_build_target_bytes <= int(build_target_bytes * 1.5) + 1, (
            f"build target grew beyond 1.5x baseline: "
            f"{third_build_target_bytes} > {build_target_bytes}"
        )
        runs_root = rust_cache / "runs"
        assert not list(runs_root.glob("*.tmp")), "transient run artifacts survived"
        assert not list(runs_root.glob("**/*output-channel*")), (
            "transient output-channel artifacts survived"
        )
        if runs_root.is_dir():
            transient_globs = (
                "**/*.profraw",
                "**/nextest.toml",
                "**/runner-map.json",
                "**/cargo-runner.toml",
                "**/instances/**",
                "**/*.tmp",
            )
            for run_dir in runs_root.iterdir():
                if not run_dir.is_dir():
                    continue
                for pattern in transient_globs:
                    matches = list(run_dir.glob(pattern))
                    assert not matches, (
                        f"run directory retained transient artifacts ({pattern}): "
                        f"{run_dir} -> {matches[:3]}"
                    )
        assert_json_integrity(rust_cache)
        log_path = archive_dir / "retained_cache_audit.log"
        log_path.write_text("\n".join(lines + ["QA PASS: retained-cache audit held."]))
        click.echo(f"Archived retained-cache audit: {log_path}")
        click.echo("QA PASS: retained-cache audit held.")


@cli.command("rust-distinct-groups-interrupt")
def rust_distinct_groups_interrupt() -> None:
    """Interrupt only after distinct nextest, shim, and delegated-child groups are live."""
    jobs = 2
    log_dir = (
        Path.home()
        / ".malvin_home"
        / "logs"
        / "d5af67e712b1a200"
        / "20260712_121211_c825yj40"
    )
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / "distinct_groups_interrupt.log"
    with qa_fixture("kiss-qa-distinct-groups-") as fixture:
        population_command = kiss_command(
            "rust",
            fixture.ignores["rust"],
            "--metrics",
            "--force",
            "-j",
            str(jobs),
        )
        interrupted, live_groups = run_interrupt_after_distinct_live_groups(
            "rust-distinct-groups-interrupt",
            population_command,
            fixture.root,
            fixture.env,
            timeout=1_800,
            repo_root=fixture.root,
        )
        assert interrupted.returncode != 0, "distinct-groups interrupt should fail"
        residual = lingering_processes_matching(
            (str(fixture.root), "rust_llvm_cov_cache")
        )
        assert not residual, f"batch descendants survived interruption: {residual}"
        recovered = run(
            "rust-distinct-groups-recover",
            population_command,
            fixture.root,
            fixture.env,
        )
        assert recovered.returncode == 0, recovered.combined
        assert metric_int(recovered.metrics(), "rust_process_residual_count") == 0
        assert_json_integrity(fixture.root / ".kiss/rust_llvm_cov_cache")
        log_path.write_text(
            "\n".join(
                [
                    f"nextest_pgid={live_groups['nextest']}",
                    f"shim_pgid={live_groups['shim']}",
                    f"delegated_pgid={live_groups['delegated']}",
                    f"interrupt_rc={interrupted.returncode}",
                    f"recover_rc={recovered.returncode}",
                    "QA PASS: distinct nextest/shim/delegated process groups "
                    "were live before interrupt; zero descendants and recoverable cache.",
                ]
            )
        )
        click.echo(f"Archived distinct-groups interrupt: {log_path}")
        click.echo(
            "QA PASS: interrupt occurred after distinct nextest, shim, and "
            "delegated-child process groups were live."
        )


def shlex_quote(value: str) -> str:
    if not value:
        return "''"
    if all(ch.isalnum() or ch in "/._-:" for ch in value):
        return value
    return "'" + value.replace("'", "'\"'\"'") + "'"


if __name__ == "__main__":
    cli()
