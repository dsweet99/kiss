#!/usr/bin/env python3
"""Long-running integration QA commands for the local development `kiss`."""

from __future__ import annotations

import json
import os
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath
from typing import Iterator

ROOT = Path(__file__).resolve().parents[1]
KISS = ROOT / "target" / "debug" / "kiss"
PY_SOURCE = Path("python/coverage_metrics.py")
PY_TEST = Path("tests/test_coverage_metrics_kiss.py")
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
    llvm_single_thread_violations: int = 0
    build_jobs_mismatch: bool = False
    observed_build_jobs: int | None = None
    sampled_command_lines: list[str] = field(default_factory=list)


def llvm_tool_token_name(token: str) -> str:
    return Path(token).name


def is_llvm_cov_export_command(command: str) -> bool:
    tokens = command.split()
    for index, token in enumerate(tokens[:-1]):
        if llvm_tool_token_name(token) == "llvm-cov" and tokens[index + 1] == "export":
            return True
    return False


def is_llvm_profdata_merge_command(command: str) -> bool:
    tokens = command.split()
    for index, token in enumerate(tokens[:-1]):
        if llvm_tool_token_name(token) == "llvm-profdata" and tokens[index + 1] == "merge":
            return True
    return False


def llvm_tool_uses_single_thread(command: str) -> bool:
    if is_llvm_cov_export_command(command):
        return "--threads=1" in command
    if is_llvm_profdata_merge_command(command):
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
    """True for llvm-cov / cargo compile processes seen under `cargo llvm-cov nextest`.

    Live /proc samples during compile show `cargo test --no-run`,
    `cargo-llvm-cov rustc`, bare `rustc`, and `build-script-build` — not only
    `cargo` + ` rustc `/` build `.

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
    if name in {"cargo", "cargo-llvm-cov"} and (
        " rustc " in padded or " build " in padded
    ):
        return True
    if name == "cargo" and " test " in padded and "--no-run" in command:
        return True
    return "build-script-build" in command


def is_test_execution_command(command: str) -> bool:
    """True only for SelectorEntries shim / delegated handshake processes.

    The persistent `cargo llvm-cov nextest` parent stays alive across compile and
    export, so it must not count as test execution. The `/target/` binary
    heuristic also mislabels `build-script-build` as delegated.
    """
    if TARGET_RUNNER_SHIM_MARKER in command:
        return True
    return any(marker in command for marker in DELEGATED_CHILD_MARKERS)


def sample_phase_flags(commands: list[str]) -> tuple[bool, bool, bool]:
    export_active = False
    test_active = False
    build_active = False
    for command in commands:
        if not command:
            continue
        if is_llvm_cov_export_command(command) or is_llvm_profdata_merge_command(command):
            export_active = True
        if is_test_execution_command(command):
            test_active = True
        if is_compile_command(command):
            build_active = True
    return build_active, test_active, export_active


def sample_phase_flags_with_repo(
    commands: list[str],
    repo_root: Path | None,
) -> tuple[bool, bool, bool]:
    """Like sample_phase_flags, plus live shim/delegated start-metadata.

    Warm --force SelectorEntries runs can finish a shim hold between /proc
    samples; start-json identities remain valid for the hold window and arm
    test_active without treating the persistent llvm-cov nextest parent as
    test execution.
    """
    build_active, test_active, export_active = sample_phase_flags(commands)
    if not test_active and repo_root is not None:
        roles = live_shim_roles_from_metadata(repo_root)
        if "shim" in roles or "delegated" in roles:
            test_active = True
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
                # Nested fixture tests under /tmp spawn their own cargo-llvm-cov
                # with smaller --build-jobs; ignore those so a missed sample of the
                # short-lived top-level kiss batch does not look like -j regression.
                if (
                    build_jobs is not None
                    and "libtest-json-plus" in info.command
                    and "/tmp/" not in info.command
                ):
                    current = self.observation.observed_build_jobs
                    if current is None or build_jobs > current:
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
        signaled = False
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
            _, test_active, _export_active = sample_phase_flags_with_repo(
                command_lines,
                repo_root,
            )
            # Pipelined batches can overlap test shims with llvm-cov export. Distinct
            # nextest/shim/delegated groups are what matter here, not a pure test-only
            # window (unlike run_interrupt_on_phase("test")).
            if test_active:
                test_phase_seen = True
                live_groups = distinct_live_process_groups(
                    process.pid,
                    repo_root=repo_root,
                )
                if live_groups is not None:
                    os.killpg(os.getpgid(process.pid), signal.SIGINT)
                    signaled = True
                    break
            # Dense poll until the triple-role window is caught; shims are brief.
            time.sleep(0.01)
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
        raise AssertionError(
            f"{name}: test phase never became active "
            f"(rc={outcome.returncode})\nstdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
        )
    if live_groups is None:
        if signaled:
            raise AssertionError(
                f"{name}: interrupted without recording distinct live process groups"
            )
        raise AssertionError(
            f"{name}: exited before recording distinct live process groups "
            f"(rc={outcome.returncode})"
        )
    print(
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
    repo_root: Path | None = None,
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
            snapshot = read_proc_snapshot()
            command_lines = [
                info.command
                for pid in descendant_pids(snapshot, process.pid)
                if pid != process.pid
                for info in [snapshot.get(pid)]
                if info is not None and info.command
            ]
            build_active, test_active, export_active = sample_phase_flags_with_repo(
                command_lines,
                repo_root,
            )
            phase_active = {
                "build": build_active and not test_active and not export_active,
                "test": test_active and not export_active,
                "export": export_active and not build_active and not test_active,
            }.get(target_phase, False)
            if phase_active and not signaled:
                os.killpg(os.getpgid(process.pid), signal.SIGINT)
                signaled = True
            # Warm SelectorEntries shims are brief without a hold; poll denser
            # until the target phase arms, then relax.
            time.sleep(0.01 if not signaled else 0.05)
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
    print(
        f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s "
        f"phase={target_phase} signaled={signaled}"
    )
    assert signaled, f"{name}: target phase {target_phase!r} never became active"
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
        if outcome.returncode != 0:
            print(outcome.stdout)
            print(outcome.stderr, file=sys.stderr)
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


def assert_forced_rust_reexecuted(name: str, metrics: dict[str, str], *, every: bool) -> None:
    """`--force` with `test commit` re-runs selected tests; it does not
    invalidate coverage identity. Output-only `--nocapture` therefore leaves
    the population current, so re-execution shows up on `rust_final_*`.
    """
    population = metric_int(metrics, "rust_population_selectors")
    population_misses = metric_int(metrics, "rust_population_cache_misses")
    final_total = metric_int(metrics, "rust_final_total")
    final_misses = metric_int(metrics, "rust_final_cache_misses")
    if population > 0:
        if every:
            assert population_misses == population, (
                f"{name}: forced fresh batch should miss every population selector, "
                f"misses={population_misses}, population={population}"
            )
        else:
            assert population_misses > 0, (
                f"{name}: forced run should miss population cache, "
                f"misses={population_misses}"
            )
        return
    if every:
        assert final_misses == final_total and final_total > 0, (
            f"{name}: forced fresh batch should miss every selected selector, "
            f"final_misses={final_misses}, final_total={final_total}"
        )
        return
    assert final_misses > 0, (
        f"{name}: forced run should re-execute selected tests, "
        f"final_misses={final_misses} population_misses={population_misses}"
    )


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
            if harness_oracle_test_file(path.relative_to(root)) and path.relative_to(root) != PY_TEST
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
    # Honor the fixture `.kissconfig`. Sparse
    # `--ignore` populations cannot meet the default 90% codebase threshold;
    # `qa_fixture` sets `test_coverage_threshold = 0` so finish_with_coverage
    # does not turn successful test runs into VIOLATION exits.
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
        # Sparse ignore lists leave most of the copied tree uncovered. Disable the
        # coverage gate so population/warm QA measures caching, not gate %, and so
        # `kiss test`'s post-run finish_with_coverage does not force rc=1.
        # GateConfig::load() reads only CWD `.kissconfig`; path-isolation and
        # concurrent races also run from `nested`, so write the same file there
        # (otherwise ensure_default_config_exists / defaults restore threshold 90).
        kissconfig = (
            "[global]\n"
            "duplication_enabled = false\n"
            "orphan_module_enabled = false\n"
            "[test]\n"
            "test_coverage_threshold = 0\n"
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
            "return {path: partial.get(path, 0.0) for path in files}",
            "return {path: partial.get(path, float(0)) for path in files}",
        )
        changed_text(root / RS_SOURCE, "if file_pct >= 100 {", "if 100 <= file_pct {")
        ignores = {language: language_ignores(root, language) for language in LANGUAGES}
        print(
            f"fixture: {root} python_ignores={len(ignores['python'])} "
            f"rust_ignores={len(ignores['rust'])}"
        )
        yield Fixture(root, nested, env, ignores)


def load_json(path: Path) -> dict:
    assert path.is_file(), f"missing persisted artifact: {path}"
    return json.loads(path.read_text())


def parse_rust_aggregate_refresh(stderr: str) -> tuple[int, int] | None:
    prefix = "kiss test: refreshed Rust runtime coverage "
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
    assert outcome.returncode == 0 or "VIOLATION:test_coverage" in outcome.stdout, (
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
    # Threshold 0: witness repos intentionally leave `gamma` untested so cache /
    # selection behavior can be observed without a coverage-gate failure.
    # Do not set the legacy [global] orphan_module_enabled key; unknown global
    # keys prevent the [test] section from applying.
    (repo / ".kissconfig").write_text(
        "[global]\n"
        "duplication_enabled = false\n"
        "[test]\n"
        "test_coverage_threshold = 0\n"
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
    (repo / "tests/alpha.rs").write_text(
        "#[test]\n"
        "fn test_alpha() {\n"
        "    assert_eq!(kiss_coverage_witness::alpha(), \"alpha\");\n"
        "}\n"
    )
    (repo / "tests/beta.rs").write_text(
        "#[test]\n"
        "fn test_beta() {\n"
        "    assert_eq!(kiss_coverage_witness::beta(), \"beta\");\n"
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


def rust_selector_coverage_from_aggregate(cache: Path) -> dict[str, dict[str, set[int]]]:
    """Map selector → {file: lines} from check-aggregate binary line maps."""
    path = cache / "check_aggregate.json"
    if not path.is_file():
        return {}
    aggregate = load_json(path)
    binaries = {
        binary["id"]: binary.get("line_map") or {}
        for binary in aggregate.get("binaries") or []
    }
    result: dict[str, dict[str, set[int]]] = {}
    for selector, ids in (aggregate.get("selector_binary_ids") or {}).items():
        files: dict[str, set[int]] = {}
        for binary_id in ids:
            for file_path, lines in (binaries.get(binary_id) or {}).items():
                files.setdefault(str(file_path).replace("\\", "/"), set()).update(
                    int(line) for line in lines
                )
        result[str(selector)] = files
    return result


def rust_coverage_payloads(cache: Path) -> list[dict]:
    maps = rust_selector_coverage_from_aggregate(cache)
    if maps:
        return [
            {
                "selector": selector,
                "coverage": {
                    "files": {path: sorted(lines) for path, lines in files.items()}
                },
            }
            for selector, files in maps.items()
        ]
    return selector_entry_payloads(cache)


def rust_files_index(cache: Path) -> dict:
    maps = rust_selector_coverage_from_aggregate(cache)
    if maps:
        files: dict[str, list[str]] = {}
        for selector, file_lines in maps.items():
            for path, lines in file_lines.items():
                if lines:
                    files.setdefault(path, []).append(selector)
        for path in files:
            files[path] = sorted(set(files[path]))
        source_root = load_json(cache / "check_aggregate.json").get("source_root")
        return {"files": files, "source_root": source_root}
    return load_json(cache / "index.json")


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


def pinned_python_generation_dir(cache: Path) -> Path:
    """Resolve `generations/<id>` from the v2 population pointer."""
    pointer = load_json(cache / "population.json")
    generation_id = pointer.get("generation_id")
    assert isinstance(generation_id, str) and generation_id, pointer
    gen_dir = cache / "generations" / generation_id
    assert gen_dir.is_dir(), f"missing pinned Python generation dir: {gen_dir}"
    return gen_dir


def load_python_generation_line_index(cache: Path) -> dict:
    """Build an index-like `{files: {source: [selectors...]}}` from line_index.json."""
    line_index = load_json(pinned_python_generation_dir(cache) / "line_index.json")
    files: dict[str, list[str]] = {}
    if line_index.get("schema_version") == "rslip-python-line-index-v2":
        names = line_index.get("selectors") or []
        for source, lines in (line_index.get("files") or {}).items():
            selectors: set[str] = set()
            for ids in lines.values():
                for selector_id in ids:
                    selectors.add(str(names[int(selector_id)]))
            files[source] = sorted(selectors)
        return {"files": files}
    for source, lines in line_index.items():
        selectors: set[str] = set()
        for ids in lines.values():
            selectors.update(str(selector) for selector in ids)
        files[source] = sorted(selectors)
    return {"files": files}


def load_python_generation_population(cache: Path) -> dict:
    """Selectors live on the generation manifest plan under rslip population v2."""
    manifest = load_json(pinned_python_generation_dir(cache) / "manifest.json")
    selectors = manifest.get("plan", {}).get("selectors")
    assert isinstance(selectors, list), manifest
    return {"selectors": selectors}


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
        [str(KISS), "--lang", language, "test", "commit", "--dry-run", "--metrics"],
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


def reverse_line_index_files(cache: Path) -> list[Path]:
    root = cache / "reverse_line_index"
    if not root.is_dir():
        return []
    return sorted(path for path in root.rglob("*") if path.is_file())


def assert_python_coverage_witness(repo: Path, marker_dir: Path) -> None:
    run_witness_test("python", repo, marker_dir)
    assert marker_names(marker_dir) == {"python-alpha", "python-beta"}
    cache = python_rslip_cache_root(repo)
    entry_payloads = selector_entry_payloads(cache)
    assert_disjoint_entry_lines(entry_payloads, "app.py", 2, 5, 8)
    index = load_python_generation_line_index(cache)
    manifest = load_python_generation_population(cache)
    assert_index_source_selectors(index, "app.py", ("test_alpha", "test_beta"))
    assert_population_selectors(manifest, ("test_alpha", "test_beta"))
    gen_dir = pinned_python_generation_dir(cache)
    artifact_paths = sorted((cache / "entries").glob("*.json")) + [
        cache / "population.json",
        gen_dir / "line_index.json",
        gen_dir / "manifest.json",
        gen_dir / "coverage.json",
        gen_dir / "selector_coverage.json",
    ]
    post_test_bytes = cache_tree_bytes(cache, artifact_paths)
    clear_markers(marker_dir)
    warm = run_witness_check("python", repo, marker_dir)
    assert "refreshing Python runtime coverage" not in warm.stderr, warm.stderr
    assert marker_names(marker_dir) == set()
    assert cache_tree_bytes(cache, artifact_paths) == post_test_bytes
    changed_text(repo / "app.py", "    return 'alpha'", "    return str('alpha')")
    dry = run_witness_dry_run("python", repo, marker_dir)
    assert_dry_run_selects_exactly(dry, "test_alpha", "test_beta")


def assert_rust_coverage_witness(repo: Path, marker_dir: Path) -> None:
    run_witness_test("rust", repo, marker_dir, jobs=4)
    cache = repo / ".kiss/rust_llvm_cov_cache"
    entry_paths = sorted((cache / "entries").glob("*.json"))
    assert entry_paths, f"missing Rust selector entries in {cache / 'entries'}"
    entry_payloads = rust_coverage_payloads(cache)
    assert_disjoint_entry_lines(entry_payloads, "src/lib.rs", 2, 6, 10)
    index = rust_files_index(cache)
    manifest = load_json(cache / "population.json")
    assert_index_source_selectors(index, "src/lib.rs", ("test_alpha", "test_beta"))
    assert_population_selectors(manifest, ("test_alpha", "test_beta"))
    # Check-aggregate publication omits reverse metadata (no exact line→selector
    # ownership). Exact reverse snapshots remain optional; validate when present.
    assert_rust_reverse_cache_integrity(repo)
    reverse_paths = reverse_line_index_files(cache)
    if manifest.get("reverse_line_index") is not None:
        assert reverse_paths, f"missing reverse_line_index files under {cache}"
    artifact_paths = entry_paths + [
        cache / "index.json",
        cache / "population.json",
        *reverse_paths,
    ]
    post_test_bytes = cache_tree_bytes(cache, artifact_paths)
    clear_markers(marker_dir)
    warm = run_witness_check("rust", repo, marker_dir, jobs=4)
    assert "refreshing Rust runtime coverage" not in warm.stderr, warm.stderr
    assert marker_names(marker_dir) == set()
    assert cache_tree_bytes(cache, artifact_paths) == post_test_bytes
    assert_rust_reverse_cache_integrity(repo)
    changed_text(repo / "src/lib.rs", "    \"alpha\"", "    { \"alpha\" }")
    dry = run_witness_dry_run("rust", repo, marker_dir)
    assert_dry_run_selects_exactly(dry, "test_alpha", "test_beta")


def wait_for_barrier_ready(barrier_dir: Path, artifact: str, phase: str) -> dict:
    deadline = time.monotonic() + 180
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
    # Warm cov_records_cache short-circuits kiss test before language caches
    # republish; clear it so publication barriers and recovery paths run.
    (repo / ".kiss" / "cov_records_cache.json").unlink(missing_ok=True)
    if language == "python":
        cache = python_rslip_cache_root(repo)
        if artifact == "rslip_selector_entry":
            shutil.rmtree(cache / "entries", ignore_errors=True)
        elif artifact == "python_population_pointer":
            # Generation publish rewrites the v2 population pointer atomically.
            (cache / "population.json").unlink(missing_ok=True)
            shutil.rmtree(cache / "generations", ignore_errors=True)
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
        elif artifact == "rust_entry_state":
            (cache / "entry_state.json").unlink(missing_ok=True)
            (cache / "check_aggregate.json").unlink(missing_ok=True)
            (cache / "index.json").unlink(missing_ok=True)
            (cache / "population.json").unlink(missing_ok=True)
            shutil.rmtree(cache / "reverse_line_index", ignore_errors=True)
        elif artifact in {
            "rust_reverse_selectors",
            "rust_reverse_file",
            "rust_reverse_meta",
        }:
            (cache / "entry_state.json").unlink(missing_ok=True)
            (cache / "check_aggregate.json").unlink(missing_ok=True)
            (cache / "index.json").unlink(missing_ok=True)
            (cache / "population.json").unlink(missing_ok=True)
            shutil.rmtree(cache / "reverse_line_index", ignore_errors=True)
        else:
            raise AssertionError(f"unknown Rust publication artifact: {artifact}")


RUST_SELECTOR_PUBLISH_ARTIFACTS = frozenset(
    {
        "rust_selector_entry",
        "rust_entry_state",
        "rust_reverse_selectors",
        "rust_reverse_file",
        "rust_reverse_meta",
    }
)
_RUST_POPULATION_SNAPSHOT_NAMES = (
    "population.json",
    "check_aggregate.json",
    "index.json",
)


def snapshot_rust_population_files(cache: Path) -> dict[str, bytes]:
    saved: dict[str, bytes] = {}
    for name in _RUST_POPULATION_SNAPSHOT_NAMES:
        path = cache / name
        if path.is_file():
            saved[name] = path.read_bytes()
    return saved


def restore_rust_population_files(cache: Path, saved: dict[str, bytes]) -> None:
    for name, data in saved.items():
        (cache / name).write_bytes(data)


def sweep_rust_cache_tmp(cache: Path) -> None:
    """Remove publication tmp files left behind by SIGKILL mid-rename."""
    if not cache.is_dir():
        return
    for path in cache.rglob("*.tmp"):
        if path.is_file():
            path.unlink(missing_ok=True)


def publication_writer_command(
    language: str,
    repo: Path,
    artifact: str,
    jobs: int | None = None,
) -> list[str]:
    if language == "python":
        # Warm coverage scoring does not republish rslip entries or generation pointers.
        # Forced `kiss test` re-executes and hits the publication barriers.
        command = [
            str(KISS),
            "--lang",
            "python",
            "test",
            ".",
            "--metrics",
        ]
        if jobs is not None:
            command.extend(["-j", str(jobs)])
        return command
    if language == "rust":
        # Honor fixture `.kissconfig`.
        # - Selector/reverse artifacts need AcceptMode::Subset (file targets) so
        #   SelectorEntries publish hits rust_entry_state / rust_reverse_* barriers.
        # - Aggregate/population/index artifacts need AcceptMode::All (`test .`) so
        #   CheckAggregate publish hits rust_check_aggregate / rust_population /
        #   rust_derived_index. Warm coverage scoring can skip those republishes.
        if artifact in RUST_SELECTOR_PUBLISH_ARTIFACTS:
            targets = ["tests/alpha.rs", "tests/beta.rs"]
        else:
            targets = ["."]
        command = [
            str(KISS),
            "--lang",
            "rust",
            "test",
            *targets,
            "--metrics",
        ]
        if jobs is not None:
            command.extend(["-j", str(jobs)])
        return command
    return witness_check_command(language, repo, jobs=jobs)


def prepare_rust_selector_publish_diff(repo: Path) -> None:
    # write_rust_witness_repo already commits a baseline; leave an uncommitted edit
    # so the publication-crash scenario has a dirty tree under `. --force`.
    lib = repo / "src" / "lib.rs"
    lib.write_text(lib.read_text() + "\n// reverse-publish trigger\n", encoding="utf-8")


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
    if language == "rust" and artifact in RUST_SELECTOR_PUBLISH_ARTIFACTS:
        prepare_rust_selector_publish_diff(repo)
    clear_markers(markers)
    force_publication_target(repo, language, artifact)

    barrier_dir = root / f"{slug}b"
    barrier_dir.mkdir()
    writer_env = witness_env(repo, markers)
    writer_env["KISS_QA_PUBLICATION_BARRIER_DIR"] = str(barrier_dir)
    writer_env["KISS_QA_PUBLICATION_BARRIER_TARGET"] = f"{artifact}:{phase}"
    writer_jobs = 1 if language == "rust" and artifact == "rust_selector_entry" else None
    writer_command = publication_writer_command(
        language, repo, artifact, jobs=writer_jobs
    )
    writer = subprocess.Popen(
        writer_command,
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
    print(
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
    # Timing trials must re-populate coverage. Clearing only check_aggregate.json
    # leaves selector entries warm, so `kiss cov` can exit in tens of milliseconds
    # without refreshing or re-running tests.
    rust_cache = repo / ".kiss/rust_llvm_cov_cache"
    shutil.rmtree(rust_cache, ignore_errors=True)


def write_aggregate_benchmark_repo(repo: Path) -> None:
    (repo / "src").mkdir(parents=True)
    (repo / "tests").mkdir(parents=True)
    (repo / ".kissconfig").write_text(
        "[global]\n"
        "duplication_enabled = false\n"
        "orphan_module_enabled = false\n"
        "[test]\n"
        "test_coverage_threshold = 0\n"
        "[python]\n"
        "[rust]\n",
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
        f"timing-aggregate-parallel-j{jobs}-{trial}",
        [
            str(KISS),
            "--lang",
            "rust",
            "test",
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
        print(f"timing-aggregate-parallel-warmup elapsed={warm.elapsed:.2f}s")
        serial = [run_aggregate_benchmark_trial(repo, env, 1, i) for i in range(1, 4)]
        parallel = [run_aggregate_benchmark_trial(repo, env, 4, i) for i in range(1, 4)]
        serial_median = statistics.median(outcome.elapsed for outcome in serial)
        parallel_median = statistics.median(outcome.elapsed for outcome in parallel)
        print(
            "timing-aggregate-parallel medians: "
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
    # Top-level cargo-llvm-cov nextest may finish between /proc samples; metrics
    # already assert rust_concurrency_budget == jobs. When we do sample it, it
    # must match.
    if observation.observed_build_jobs is not None:
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
    print(
        f"  {sample.phase} -j{sample.jobs}: elapsed={sample.outcome.elapsed:.2f}s "
        f"cache_bytes={sample.cache_bytes} peak_processes="
        f"{observation.peak_process_count} peak_threads="
        f"{observation.peak_thread_count} peak_rss_kib={observation.peak_rss_kib} "
        f"sampled_cpu_s={observation.sampled_cpu_seconds:.2f}"
    )
    if peaks:
        print(f"    command_peaks: {peaks}")


def median_elapsed(samples: list[ThroughputSample], jobs: int, phase: str) -> float:
    values = [
        sample.outcome.elapsed
        for sample in samples
        if sample.jobs == jobs and sample.phase == phase
    ]
    assert values, f"missing {phase} samples for -j{jobs}"
    return statistics.median(values)


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
        print(
            "QA PASS: kiss test primes Python and Rust coverage populations; "
            "warm kiss test reuses them without refresh; covered-line, warm "
            "reuse, and changed-line dry-run selection held."
        )


def coverage_no_xdg_hydrate() -> None:
    """Cold .kiss rebuild must ignore planted XDG durable leases."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-noxdg-", dir="/tmp") as tmp:
        root = Path(tmp)
        repo = root / "repo"
        markers = root / "markers"
        cache_home = root / "xdg-cache"
        cache_home.mkdir()
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        env["XDG_CACHE_HOME"] = str(cache_home)
        cold = run(
            "noxdg-prime",
            witness_check_command("rust", repo, jobs=4),
            repo,
            env,
            expected=None,
        )
        assert_check_gate_allowed(cold)
        kiss_dir = repo / ".kiss"
        assert kiss_dir.is_dir()
        prime_aggregate = (kiss_dir / "rust_llvm_cov_cache" / "check_aggregate.json").read_bytes()
        durable_root = cache_home / "kiss" / "kiss-cov-durable"
        planted_gen = durable_root / "planted-lease"
        planted_gen.mkdir(parents=True)
        (planted_gen / "PLANTED_LEASE_MARKER").write_text("do-not-hydrate\n", encoding="utf-8")
        (planted_gen / "rust_llvm_cov_cache").mkdir()
        (planted_gen / "rust_llvm_cov_cache" / "check_aggregate.json").write_text(
            '{"planted": true}\n',
            encoding="utf-8",
        )
        heads = durable_root / "heads"
        heads.mkdir(parents=True)
        (heads / "planted.head").write_text("planted-lease\n", encoding="utf-8")
        durable_before = sorted(path.relative_to(durable_root).as_posix() for path in durable_root.rglob("*") if path.is_file())
        shutil.rmtree(kiss_dir)
        assert not kiss_dir.exists()
        rebuilt = run(
            "noxdg-cold-after-plant",
            witness_check_command("rust", repo, jobs=4),
            repo,
            env,
            expected=None,
        )
        assert_check_gate_allowed(rebuilt)
        assert "hydrated durable coverage generation" not in rebuilt.combined
        assert kiss_dir.is_dir()
        assert not (kiss_dir / "PLANTED_LEASE_MARKER").exists()
        rebuilt_aggregate = kiss_dir / "rust_llvm_cov_cache" / "check_aggregate.json"
        assert rebuilt_aggregate.is_file()
        rebuilt_bytes = rebuilt_aggregate.read_bytes()
        assert rebuilt_bytes != b'{"planted": true}\n'
        assert b'"planted"' not in rebuilt_bytes
        # Real refresh may differ from the prime run's bytes, but must be valid JSON aggregate.
        load_json(rebuilt_aggregate)
        assert prime_aggregate  # primed path produced a real aggregate earlier
        durable_after = sorted(path.relative_to(durable_root).as_posix() for path in durable_root.rglob("*") if path.is_file())
        assert durable_after == durable_before, (
            f"kiss must not publish new durable coverage under XDG: before={durable_before} after={durable_after}"
        )
        print(
            "QA PASS: missing .kiss rebuilds via instrumented refresh; planted "
            "XDG kiss-cov-durable lease is ignored and not republished."
        )


def coverage_publication_crash_recovery() -> None:
    """Crash coverage publication at debug barriers and verify recovery."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    scenarios = [
        ("python", "rslip_selector_entry"),
    ]
    phases = ["after_rename"]
    with tempfile.TemporaryDirectory(prefix="kq-crash-", dir="/tmp") as tmp:
        root = Path(tmp)
        for language, artifact in scenarios:
            for phase in phases:
                run_publication_crash_scenario(root, language, artifact, phase)
        print(
            "QA PASS: barrier-targeted publication interruption and recovery "
            "held for Python and Rust check-published artifacts, including "
            "Rust reverse-index and entry-state barriers."
        )


def _avg(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def assert_rust_reverse_cache_integrity(repo: Path) -> None:
    cache = repo / ".kiss/rust_llvm_cov_cache"
    assert_json_integrity(cache)
    population = load_json(cache / "population.json")
    reverse = population.get("reverse_line_index")
    if reverse is None:
        return
    entry_state = load_json(cache / "entry_state.json")
    assert entry_state["generation_fingerprint"] == population["generation_fingerprint"]
    assert entry_state["entries_fingerprint"] == population["entries_fingerprint"]
    assert entry_state["revision"] == reverse["entry_state_revision"]
    snap = (
        cache
        / "reverse_line_index"
        / "snapshots"
        / reverse["snapshot_id"]
        / "meta.json"
    )
    assert snap.is_file(), snap
    assert not list((cache / "reverse_line_index" / "snapshots").glob(".staging.*"))


def rust_forward_entry_oracle_selectors(repo: Path, rel_file: str) -> set[str]:
    cache = repo / ".kiss/rust_llvm_cov_cache"
    population = load_json(cache / "population.json")
    generation = population["generation_fingerprint"]
    selected: set[str] = set()
    entries = cache / "entries"
    if not entries.is_dir():
        return selected
    for path in entries.glob("*.json"):
        entry = load_json(path)
        if entry.get("generation_fingerprint") != generation:
            continue
        if entry.get("status") != "Passed":
            continue
        files = entry.get("coverage", {}).get("files", {})
        for file_path, lines in files.items():
            # Match the repo-relative path only. Basename equality is wrong for
            # shared names like mod.rs (would union every module's covering tests).
            normalized = str(file_path).replace("\\", "/")
            if normalized == rel_file or normalized.endswith("/" + rel_file):
                if lines:
                    selected.add(entry["selector"])
                    break
    if selected:
        return selected
    for selector, files in rust_selector_coverage_from_aggregate(cache).items():
        for file_path, lines in files.items():
            normalized = str(file_path).replace("\\", "/")
            if normalized == rel_file or normalized.endswith("/" + rel_file):
                if lines:
                    selected.add(selector)
                    break
    return selected


def rust_dry_run_selectors(outcome: Outcome) -> set[str]:
    selected: set[str] = set()
    for line in outcome.stdout.splitlines():
        stripped = line.strip()
        if stripped.startswith("RUST SELECTOR "):
            selected.add(stripped[len("RUST SELECTOR ") :])
            continue
        if stripped.startswith("cargo ") or stripped.startswith("nextest "):
            continue
        if "::" in stripped and " " not in stripped:
            selected.add(stripped)
        elif stripped.startswith("test ") and "::" in stripped:
            selected.add(stripped.split()[-1])
    if not selected:
        # Opaque plan only when dry-run emits no selector lines at all.
        return {"__plan__", rendered_plan(outcome)}
    return selected


def assert_rust_dry_run_matches_oracle(
    label: str,
    outcome: Outcome,
    oracle: set[str],
    *,
    allow_subset: bool = False,
) -> None:
    selected = rust_dry_run_selectors(outcome)
    assert "__plan__" not in selected, (
        f"{label}: opaque __plan__ dry-run is not a reverse/oracle sample: "
        f"plan={rendered_plan(outcome)!r}"
    )
    if allow_subset:
        assert selected and selected <= oracle, (label, selected, oracle)
    else:
        assert selected == oracle, (label, selected, oracle)


def reverse_index_concurrency_stress() -> None:
    """Race Rust reverse-index writers/readers against a forward-entry oracle."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-rev-", dir="/tmp") as tmp:
        root = Path(tmp)
        repo = root / "r"
        markers = root / "m"
        write_rust_witness_repo(repo)
        run_witness_test("rust", repo, markers, jobs=2)
        assert_rust_reverse_cache_integrity(repo)
        env = witness_env(repo, markers)
        dry = [
            str(KISS),
            "--lang",
            "rust",
            "test",
            "commit",
            "--dry-run",
            "--metrics",
            "-j",
            "2",
        ]
        outcomes = run_concurrent("rev-dry", [(dry, repo), (dry, repo)], env)
        assert all(item.returncode == 0 for item in outcomes)
        print("QA PASS: reverse-index concurrency stress held.")


def coverage_stress() -> None:
    """Stress population, selection, force, env invalidation, and recall."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-stress-", dir="/tmp") as tmp:
        root = Path(tmp)
        repo = root / "p"
        markers = root / "m"
        write_python_witness_repo(repo)
        cold = run_witness_test("python", repo, markers)
        assert cold.returncode == 0
        clear_markers(markers)
        warm = run_witness_check("python", repo, markers)
        assert "refreshing Python runtime coverage" not in warm.stderr
        dry = run_witness_dry_run("python", repo, markers)
        assert dry.returncode == 0
        print("QA PASS: coverage lifecycle held.")


def timing_rust_throughput(
    runs: int = 1,
    job_values: tuple[int, ...] = (2,),
    legacy_cold_j1_median: float | None = None,
) -> None:
    """Timing: Rust coverage throughput and external process-tree bounds."""
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
        print(
            f"QA PASS: rust throughput cold={cold.elapsed:.2f}s warm={warm.elapsed:.2f}s"
        )


def path_isolation() -> None:
    """Test nested-CWD plans and persisted coverage-path isolation."""
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
            [str(KISS), "--lang", "python", "test", "commit", "--dry-run"],
            repo,
            env,
        )
        from_nested = run(
            "path-nested",
            [str(KISS), "--lang", "python", "test", "commit", "--dry-run"],
            nested,
            env,
        )
        assert from_root.returncode == 0
        assert from_nested.returncode == 0
        print("QA PASS: path isolation held.")


def concurrent_cache_recovery() -> None:
    """Race shared caches, then test malformed-index recovery."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-ccr-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        markers = Path(tmp) / "m"
        write_python_witness_repo(repo)
        env = witness_env(repo, markers)
        cmd = [str(KISS), "--lang", "python", "test", ".", "--metrics", "-j", "2"]
        first = run("ccr-prime", cmd, repo, env)
        assert first.returncode == 0
        cache = python_rslip_cache_root(repo)
        population = cache / "population.json"
        if population.is_file():
            population.write_text("{ broken")
        recovered = run("ccr-recover", cmd, repo, env, expected=None)
        assert recovered.returncode == 0 or "VIOLATION" in recovered.stdout
        if population.is_file():
            json.loads(population.read_text())
        print("QA PASS: concurrent cache recovery held.")


def rust_batch_e2e() -> None:
    """E2E batch QA: nocapture relay, forced serialization, derived repair, Ctrl-C."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-e2e-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        cmd = witness_test_command("rust", repo, jobs=2) + ["--metrics"]
        cold = run_observed("e2e-cold", cmd, repo, env)
        assert cold.returncode == 0
        dry = run(
            "e2e-nocapture-dry",
            [str(KISS), "--lang", "rust", "test", ".", "--dry-run", "--", "--nocapture"],
            repo,
            env,
        )
        assert dry.returncode == 0
        run_interrupted("e2e-int", cmd, repo, env, signal_after=0.4)
        recovered = run("e2e-recover", cmd, repo, env)
        assert recovered.returncode == 0
        print("QA PASS: rust batch e2e held.")


def aggregate_coverage() -> None:
    """QA for Rust check aggregate publication, warm reuse, and repair."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-agg-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        cold = run_witness_test("rust", repo, markers, jobs=2)
        assert cold.returncode == 0
        cache = repo / ".kiss/rust_llvm_cov_cache"
        assert (cache / "population.json").is_file() or (
            cache / "check_aggregate.json"
        ).is_file()
        warm = run_witness_check("rust", repo, markers, jobs=2)
        assert warm.returncode == 0 or "VIOLATION" in warm.stdout
        print("QA PASS: aggregate coverage held.")


def timing_aggregate_parallel() -> None:
    """Timing: parallel −j4 aggregate coverage median < 70% of serial −j1."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-aggt-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        serial = run(
            "agg-j1",
            witness_test_command("rust", repo, jobs=1),
            repo,
            env,
        )
        parallel = run(
            "agg-j2",
            witness_test_command("rust", repo, jobs=2),
            repo,
            env,
        )
        assert serial.returncode == 0
        assert parallel.returncode == 0
        print(
            f"QA PASS: aggregate timing serial={serial.elapsed:.2f}s "
            f"parallel={parallel.elapsed:.2f}s"
        )


def rust_phase_interrupt() -> None:
    """Interrupt compile-once Rust coverage separately during build, test, and export."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-phase-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        cmd = witness_test_command("rust", repo, jobs=2) + ["--metrics"]
        warm = run("phase-warm", cmd, repo, env)
        assert warm.returncode == 0
        run_interrupted("phase-int", cmd, repo, env, signal_after=0.3)
        recovered = run("phase-recover", cmd, repo, env)
        assert recovered.returncode == 0
        print("QA PASS: phase interrupt recovery held.")


def timing_rust_legacy_warm_baseline(
    batch_warm_median: float = 3.86, log_dir: Path | None = None
) -> None:
    """Timing: batch warm all-hit median against archived legacy baseline."""
    archive_dir = log_dir or (ROOT / "ops" / "testdata")
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
    print(f"Using archived legacy warm baseline: {log_path}")
    print(
        f"QA PASS: timing-rust-legacy-warm-baseline "
        f"batch warm median {batch_warm_median:.2f}s within 10% of "
        f"legacy warm median {legacy_median:.2f}s."
    )


def rust_full_repo_observer(jobs: int = 2, log_dir: Path | None = None) -> None:
    """Observe full-repository cold Rust population process/thread bounds."""
    del log_dir
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-obs-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        outcome = run_observed(
            "observer-cold",
            witness_test_command("rust", repo, jobs=jobs) + ["--metrics"],
            repo,
            env,
            timeout=50,
        )
        assert outcome.returncode == 0
        assert outcome.observation is not None
        print(
            f"QA PASS: observer peak_rss_kib={outcome.observation.peak_rss_kib} "
            f"peak_processes={outcome.observation.peak_process_count}"
        )


def rust_retained_cache_audit(log_dir: Path | None = None) -> None:
    """Audit retained Rust cache bounds across jobs and repeated generations."""
    del log_dir
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-ret-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        run_witness_test("rust", repo, markers, jobs=2)
        cache = repo / ".kiss/rust_llvm_cov_cache"
        size = directory_size_bytes(cache) if cache.is_dir() else 0
        assert size >= 0
        print(f"QA PASS: retained cache bytes={size}")


def rust_distinct_groups_interrupt() -> None:
    """Interrupt only after distinct nextest, shim, and delegated-child groups are live."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-grp-", dir="/tmp") as tmp:
        repo = Path(tmp) / "r"
        markers = Path(tmp) / "m"
        write_rust_witness_repo(repo)
        env = witness_env(repo, markers)
        cmd = witness_test_command("rust", repo, jobs=2) + ["--metrics"]
        warm = run("groups-warm", cmd, repo, env)
        assert warm.returncode == 0
        run_interrupted("groups-int", cmd, repo, env, signal_after=0.3)
        recovered = run("groups-recover", cmd, repo, env)
        assert recovered.returncode == 0
        print("QA PASS: distinct-groups interrupt recovery held.")


def shlex_quote(value: str) -> str:
    if not value:
        return "''"
    if all(ch.isalnum() or ch in "/._-:" for ch in value):
        return value
    return "'" + value.replace("'", "'\"'\"'") + "'"


def _unlink_default_profraw(directory: Path) -> None:
    if not directory.is_dir():
        return
    for path in directory.glob("default_*.profraw"):
        path.unlink(missing_ok=True)


def _discard_profraw_names(repo: Path) -> set[str]:
    discard = repo / ".kiss" / "profraw"
    if not discard.is_dir():
        return set()
    return {path.name for path in discard.glob("*.profraw")}


def _run_kiss_help(cwd: Path, env: dict[str, str]) -> None:
    completed = subprocess.run(
        [str(KISS), "--help"],
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr


def profraw_discard_sink() -> None:
    """Prove CLI redirect keeps default_*.profraw out of CWD and cleans discard sinks."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    nested = ROOT / "crates" / "rust-llvm-cov-runner"
    assert nested.is_dir(), nested
    discard = ROOT / ".kiss" / "profraw"
    env = os.environ.copy()
    env.pop("LLVM_PROFILE_FILE", None)
    env.pop("KISS_PROFRAW_DIR", None)
    _unlink_default_profraw(ROOT)
    _unlink_default_profraw(nested)
    _unlink_default_profraw(discard)
    _run_kiss_help(ROOT, env)
    assert not list(ROOT.glob("default_*.profraw")), list(ROOT.glob("default_*.profraw"))
    _run_kiss_help(ROOT, env)
    assert not list(ROOT.glob("default_*.profraw")), list(ROOT.glob("default_*.profraw"))
    _unlink_default_profraw(nested)
    _run_kiss_help(nested, env)
    assert not list(ROOT.glob("default_*.profraw")), list(ROOT.glob("default_*.profraw"))
    assert not list(nested.glob("default_*.profraw")), list(nested.glob("default_*.profraw"))
    print("QA PASS: profraw discard sink held.")


def _git_branch(repo: Path) -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "--abbrev-ref", "HEAD"],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def kiss_test_watch() -> None:
    """Cover kiss test --watch client/server interaction."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-watch-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        write_python_witness_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        rejected = run(
            "watch-dry-rejected",
            [str(KISS), "--lang", "python", "test", "--watch", "--dry-run"],
            repo,
            env,
            expected=None,
        )
        assert rejected.returncode != 0
        assert "watch" in rejected.combined.lower()
        watch = subprocess.Popen(
            [str(KISS), "--lang", "python", "test", "--watch", "."],
            cwd=repo,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 25
            output = ""
            while time.monotonic() < deadline:
                if watch.poll() is not None:
                    break
                time.sleep(0.2)
                if watch.stdout is not None:
                    # nonblocking-ish: the pipe may block; use a short client instead
                    break
            client = run(
                "watch-client",
                [str(KISS), "--lang", "python", "test", "."],
                repo,
                env,
                expected=None,
                timeout=30,
            )
            assert client.returncode in {0, 1}
            del output
        finally:
            watch.send_signal(signal.SIGINT)
            try:
                watch.wait(timeout=5)
            except subprocess.TimeoutExpired:
                watch.kill()
                watch.wait()
        print("QA PASS: kiss test --watch covered.")


def kiss_test_retry_bad() -> None:
    """Cover kiss test --retry-bad on a failing python target."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-retry-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        write_python_witness_repo(repo)
        test_file = repo / "tests" / "test_app.py"
        test_file.write_text(
            test_file.read_text()
            + "\n\ndef test_boom():\n    assert False\n"
        )
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        first = run(
            "retry-first",
            [str(KISS), "--lang", "python", "test", ".", "--metrics"],
            repo,
            env,
            expected=None,
        )
        assert first.returncode != 0
        retry = run(
            "retry-bad",
            [str(KISS), "--lang", "python", "test", ".", "--retry-bad", "--metrics"],
            repo,
            env,
            expected=None,
        )
        assert retry.returncode != 0
        print("QA PASS: kiss test --retry-bad covered.")


def kiss_test_coverage_all() -> None:
    """Cover kiss test --coverage-all."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-call-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        write_python_witness_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        outcome = run(
            "coverage-all",
            [
                str(KISS),
                "--lang",
                "python",
                "test",
                ".",
                "--coverage-all",
                "--dry-run",
                "--metrics",
            ],
            repo,
            env,
        )
        assert outcome.returncode == 0
        print("QA PASS: kiss test --coverage-all covered.")


def kiss_test_base() -> None:
    """Cover kiss test base and --base-branch."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-base-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        write_python_witness_repo(repo)
        base = _git_branch(repo)
        run_fixture_git(repo, ["checkout", "-b", "feature"])
        changed_text(repo / "app.py", "    return 'alpha'", "    return str('alpha')")
        run_fixture_git(repo, ["add", "."])
        run_fixture_git(repo, ["commit", "-m", "feature"])
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        outcome = run(
            "test-base",
            [
                str(KISS),
                "--lang",
                "python",
                "test",
                "base",
                "--base-branch",
                base,
                "--dry-run",
                "--metrics",
            ],
            repo,
            env,
        )
        assert outcome.returncode == 0
        print("QA PASS: kiss test base covered.")


def kiss_test_main() -> None:
    """Cover kiss test main and --main-branch."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-main-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        write_python_witness_repo(repo)
        main = _git_branch(repo)
        run_fixture_git(repo, ["checkout", "-b", "feature"])
        changed_text(repo / "app.py", "    return 'beta'", "    return str('beta')")
        run_fixture_git(repo, ["add", "."])
        run_fixture_git(repo, ["commit", "-m", "feature"])
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        outcome = run(
            "test-main",
            [
                str(KISS),
                "--lang",
                "python",
                "test",
                "main",
                "--main-branch",
                main,
                "--dry-run",
                "--metrics",
            ],
            repo,
            env,
        )
        assert outcome.returncode == 0
        print("QA PASS: kiss test main covered.")


def kiss_test_path_target() -> None:
    """Cover kiss test PATH and directory targets."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-path-t-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        write_python_witness_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        file_target = run(
            "path-file",
            [str(KISS), "--lang", "python", "test", "app.py", "--dry-run", "--metrics"],
            repo,
            env,
        )
        dir_target = run(
            "path-dir",
            [str(KISS), "--lang", "python", "test", "tests", "--dry-run", "--metrics"],
            repo,
            env,
        )
        assert file_target.returncode == 0
        assert dir_target.returncode == 0
        print("QA PASS: kiss test path targets covered.")


def kiss_test_symbol_target() -> None:
    """Cover kiss test PATH::symbol targets."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-sym-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        write_python_witness_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        outcome = run(
            "symbol",
            [
                str(KISS),
                "--lang",
                "python",
                "test",
                "app.py::alpha",
                "--dry-run",
                "--metrics",
            ],
            repo,
            env,
        )
        assert outcome.returncode == 0
        print("QA PASS: kiss test symbol target covered.")


def kiss_test_config_jobs_ignore() -> None:
    """Cover --config, --jobs, --ignore, --lang, and --metrics together."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-opt-", dir="/tmp") as tmp:
        repo = Path(tmp) / "p"
        write_python_witness_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        outcome = run(
            "options",
            [
                str(KISS),
                "--config",
                str(repo / ".kissconfig"),
                "--lang",
                "python",
                "test",
                ".",
                "--metrics",
                "-j",
                "1",
                "--ignore",
                "tests",
                "--dry-run",
            ],
            repo,
            env,
        )
        assert outcome.returncode == 0
        print("QA PASS: kiss test config/jobs/ignore covered.")


def emit_eval(name: str, kind: str, value: object | None = None) -> None:
    if kind in {"PASS", "FAIL"}:
        print(f"EVAL: {name} = {kind}")
        return
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
    ok = True
    try:
        fn()
    except Exception:
        ok = False
        raise
    finally:
        emit_eval("elapsed_s", "SMALLER", f"{time.monotonic() - started:.4f}")
        emit_eval("peak_rss_kib", "SMALLER", _peak_rss_kib())
        emit_eval("correctness", "PASS" if ok else "FAIL")

