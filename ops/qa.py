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
    click.echo(
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
    # Honor the fixture `.kissconfig` (do not pass `--defaults`). Sparse
    # `--ignore` populations cannot meet the default 90% codebase threshold;
    # `qa_fixture` sets `test_coverage_threshold = 0` so finish_with_coverage
    # does not turn successful test runs into GATE_FAILED exits.
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
            "[gate]\n"
            "test_coverage_threshold = 0\n"
            "duplication_enabled = false\n"
            "orphan_module_enabled = false\n"
            "[gate.max_unit_test_seconds]\n"
            '"*" = 30\n'
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
        click.echo(
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
    # Threshold 0: witness repos intentionally leave `gamma` untested so cache /
    # selection behavior can be observed without a coverage-gate failure.
    (repo / ".kissconfig").write_text(
        "[gate]\n"
        "test_coverage_threshold = 0\n"
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
    # Honor the fixture `.kissconfig` (do not pass `--defaults`).
    command = [str(KISS), "--lang", language, "__coverage"]
    if jobs is not None:
        command.extend(["-j", str(jobs)])
    command.append(str(repo))
    return command


def witness_test_command(language: str, repo: Path, jobs: int | None = None) -> list[str]:
    # Honor the fixture `.kissconfig` (do not pass `--defaults`).
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
    assert marker_names(marker_dir) == {"rust-alpha", "rust-beta"}
    cache = repo / ".kiss/rust_llvm_cov_cache"
    entry_paths = sorted((cache / "entries").glob("*.json"))
    assert entry_paths, f"missing Rust selector entries in {cache / 'entries'}"
    entry_payloads = [load_json(path) for path in entry_paths]
    assert_disjoint_entry_lines(entry_payloads, "src/lib.rs", 2, 6, 10)
    index = load_json(cache / "index.json")
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
    assert metric_int(dry.metrics(), "selected_rust_initial") == 1


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


def publication_writer_command(
    language: str,
    repo: Path,
    artifact: str,
    jobs: int | None = None,
) -> list[str]:
    if language == "python":
        # Warm `__coverage` does not republish rslip entries or generation pointers.
        # Forced `kiss test` re-executes and hits the publication barriers.
        command = [
            str(KISS),
            "--lang",
            "python",
            "test",
            ".",
            "--force",
            "--metrics",
        ]
        if jobs is not None:
            command.extend(["-j", str(jobs)])
        return command
    if language == "rust":
        # Honor fixture `.kissconfig` (no `--defaults`).
        # - Selector/reverse artifacts need AcceptMode::Subset (file targets) so
        #   SelectorEntries publish hits rust_entry_state / rust_reverse_* barriers.
        # - Aggregate/population/index artifacts need AcceptMode::All (`test .`) so
        #   CheckAggregate publish hits rust_check_aggregate / rust_population /
        #   rust_derived_index. Warm `__coverage` can skip those republishes.
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
            "--force",
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
    # Timing trials must re-populate coverage. Clearing only check_aggregate.json
    # leaves selector entries warm, so `kiss cov` can exit in tens of milliseconds
    # without refreshing or re-running tests.
    rust_cache = repo / ".kiss/rust_llvm_cov_cache"
    shutil.rmtree(rust_cache, ignore_errors=True)


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
        f"timing-aggregate-parallel-j{jobs}-{trial}",
        [
            str(KISS),
            "--defaults",
            "--lang",
            "rust",
            "__coverage",
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
        click.echo(f"timing-aggregate-parallel-warmup elapsed={warm.elapsed:.2f}s")
        serial = [run_aggregate_benchmark_trial(repo, env, 1, i) for i in range(1, 4)]
        parallel = [run_aggregate_benchmark_trial(repo, env, 4, i) for i in range(1, 4)]
        serial_median = statistics.median(outcome.elapsed for outcome in serial)
        parallel_median = statistics.median(outcome.elapsed for outcome in parallel)
        click.echo(
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
            "QA PASS: kiss test primes Python and Rust coverage populations; "
            "warm kiss test reuses them without refresh; covered-line, warm "
            "reuse, and changed-line dry-run selection held."
        )


@cli.command("coverage-no-xdg-hydrate")
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
        assert "refreshing Rust runtime coverage" in cold.stderr, cold.stderr
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
        assert "refreshing Rust runtime coverage" in rebuilt.stderr, rebuilt.stderr
        assert "hydrated durable coverage generation" not in rebuilt.stderr, rebuilt.stderr
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
        click.echo(
            "QA PASS: missing .kiss rebuilds via instrumented refresh; planted "
            "XDG kiss-cov-durable lease is ignored and not republished."
        )


@cli.command("coverage-publication-crash-recovery")
def coverage_publication_crash_recovery() -> None:
    """Crash coverage publication at debug barriers and verify recovery."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    scenarios = [
        ("python", "rslip_selector_entry"),
        ("python", "python_population_pointer"),
        ("rust", "rust_selector_entry"),
        ("rust", "rust_entry_state"),
        ("rust", "rust_derived_index"),
        ("rust", "rust_reverse_selectors"),
        ("rust", "rust_reverse_file"),
        ("rust", "rust_reverse_meta"),
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
            "held for Python and Rust check-published artifacts, including "
            "Rust reverse-index and entry-state barriers."
        )


@cli.command("reverse-index-concurrency-stress")
def reverse_index_concurrency_stress() -> None:
    """Race Rust reverse-index writers/readers against a forward-entry oracle."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    jobs = 2
    iterations = 20
    writer_slots = 8
    reader_count = 16
    with qa_fixture("kiss-qa-reverse-stress-") as fixture:
        # Full empty-cache population uses CheckAggregate (no reverse). A follow-up
        # --force on the edited Rust source remasures via SelectorEntries and
        # activates reverse_line_index for the oracle races below.
        cold = run(
            "rust-cold-population",
            kiss_command(
                "rust",
                fixture.ignores["rust"],
                "--metrics",
                "-j",
                str(jobs),
            ),
            fixture.root,
            fixture.env,
        )
        assert_metric(cold.metrics(), "rust_population_required", "true")
        reverse_prime = run(
            "rust-reverse-prime",
            [
                str(KISS),
                "--lang",
                "rust",
                "test",
                RS_SOURCE.as_posix(),
                "--force",
                "--metrics",
                "-j",
                str(jobs),
            ],
            fixture.root,
            fixture.env,
        )
        assert reverse_prime.returncode == 0, reverse_prime.stderr
        assert_metric(reverse_prime.metrics(), "rust_population_required", "false")
        population = load_json(
            fixture.root / ".kiss/rust_llvm_cov_cache" / "population.json"
        )
        assert population.get("reverse_line_index") is not None, population
        assert_rust_reverse_cache_integrity(fixture.root)

        rel = RS_SOURCE.as_posix()
        symbol = f"{rel}::format_unreferenced_unit_coverage_message"
        hit_latencies_ms: list[float] = []
        fallback_latencies_ms: list[float] = []
        killed_writers = 0
        repaired = 0

        for iteration in range(iterations):
            writers: list[tuple[list[str], Path]] = []
            cwds = [fixture.root, fixture.nested] * (writer_slots // 2)
            for cwd in cwds:
                writers.append(
                    (
                        kiss_command(
                            "rust",
                            fixture.ignores["rust"],
                            "--metrics",
                            "-j",
                            str(jobs),
                        ),
                        cwd,
                    )
                )

            # Inject a publication-barrier kill on selected iterations across
            # reverse publish barriers (not only rust_population).
            kill_this_round = iteration in (0, 5, 10, 15)
            if kill_this_round:
                reverse_barriers = [
                    ("rust_entry_state", "after_sync_before_rename"),
                    ("rust_reverse_selectors", "after_sync_before_rename"),
                    ("rust_reverse_file", "after_sync_before_rename"),
                    ("rust_reverse_meta", "after_sync_before_rename"),
                    ("rust_population", "after_sync_before_rename"),
                    ("rust_population", "after_rename"),
                ]
                artifact, phase = reverse_barriers[iteration % len(reverse_barriers)]
                barrier_dir = fixture.root / f".kiss-qa-barrier-{iteration}"
                barrier_dir.mkdir(exist_ok=True)
                force_publication_target(fixture.root, "rust", artifact)
                writer_env = dict(fixture.env)
                writer_env["KISS_QA_PUBLICATION_BARRIER_DIR"] = str(barrier_dir)
                writer_env["KISS_QA_PUBLICATION_BARRIER_TARGET"] = f"{artifact}:{phase}"
                kill_cmd = kiss_command(
                    "rust",
                    fixture.ignores["rust"],
                    "--force",
                    "--metrics",
                    "-j",
                    "1",
                )
                writer = subprocess.Popen(
                    kill_cmd,
                    cwd=fixture.root,
                    env=writer_env,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    start_new_session=True,
                )
                try:
                    wait_for_barrier_ready(barrier_dir, artifact, phase)
                    os.killpg(os.getpgid(writer.pid), signal.SIGKILL)
                    killed_writers += 1
                finally:
                    writer.communicate(timeout=60)
                # Repair after kill.
                repair = run(
                    f"reverse-repair-{iteration}",
                    kiss_command(
                        "rust",
                        fixture.ignores["rust"],
                        "--force",
                        "--metrics",
                        "-j",
                        str(jobs),
                    ),
                    fixture.root,
                    fixture.env,
                )
                assert repair.returncode == 0, repair.stderr
                repaired += 1
                assert_rust_reverse_cache_integrity(fixture.root)

            writer_outcomes = run_concurrent(
                f"reverse-writers-{iteration}", writers, fixture.env
            )
            for outcome in writer_outcomes:
                assert outcome.returncode == 0, outcome.stderr

            file_readers: list[tuple[list[str], Path]] = []
            symbol_readers: list[tuple[list[str], Path]] = []
            for i in range(reader_count // 2):
                cwd = fixture.root if i % 2 == 0 else fixture.nested
                shared = [
                    str(KISS),
                    "--defaults",
                    "--lang",
                    "rust",
                    "test",
                    "PLACEHOLDER",
                    "--dry-run",
                    "--metrics",
                    "-j",
                    str(jobs),
                ]
                file_cmd = list(shared)
                file_cmd[5] = rel
                symbol_cmd = list(shared)
                symbol_cmd[5] = symbol
                file_readers.append((file_cmd, cwd))
                symbol_readers.append((symbol_cmd, cwd))
            t0 = time.monotonic()
            file_outcomes = run_concurrent(
                f"reverse-file-readers-{iteration}", file_readers, fixture.env
            )
            symbol_outcomes = run_concurrent(
                f"reverse-symbol-readers-{iteration}", symbol_readers, fixture.env
            )
            elapsed_ms = (time.monotonic() - t0) * 1000.0
            for outcome in file_outcomes + symbol_outcomes:
                assert outcome.returncode == 0, outcome.stderr
            assert len({rendered_plan(o) for o in file_outcomes}) == 1
            assert len({rendered_plan(o) for o in symbol_outcomes}) == 1
            oracle = rust_forward_entry_oracle_selectors(fixture.root, rel)
            assert oracle, f"forward oracle empty for {rel}"
            for outcome in file_outcomes:
                assert_rust_dry_run_matches_oracle("file", outcome, oracle)
            for outcome in symbol_outcomes:
                assert_rust_dry_run_matches_oracle(
                    "symbol", outcome, oracle, allow_subset=True
                )
            hit_latencies_ms.append(elapsed_ms)

            # Reverse-hit probe: entries unreadable ⇒ answer must still equal oracle.
            cache = fixture.root / ".kiss/rust_llvm_cov_cache"
            entries = cache / "entries"
            assert entries.is_dir(), entries
            mode = entries.stat().st_mode
            try:
                entries.chmod(0o000)
                probe = run(
                    f"reverse-hit-zero-entry-{iteration}",
                    [
                        str(KISS),
                        "--defaults",
                        "--lang",
                        "rust",
                        "test",
                        rel,
                        "--dry-run",
                        "--metrics",
                        "-j",
                        str(jobs),
                    ],
                    fixture.root,
                    fixture.env,
                )
                assert probe.returncode == 0, (
                    "reverse-hit dry-run must succeed with entries/ unreadable "
                    f"(zero entry-file reads): {probe.stderr}"
                )
                assert_rust_dry_run_matches_oracle(
                    f"zero-entry-reverse-hit-{iteration}", probe, oracle
                )
            finally:
                entries.chmod(mode)

            assert_rust_reverse_cache_integrity(fixture.root)
            click.echo(
                f"reverse stress iteration {iteration}: plan_ok oracle_ok "
                f"elapsed_ms={elapsed_ms:.1f}"
            )

        assert hit_latencies_ms, (
            "reverse-index stress produced no reverse-hit/oracle samples "
            "(opaque __plan__-only dry-runs are not enough)"
        )
        click.echo(
            "QA PASS: reverse-index concurrency stress held "
            f"({iterations} iterations, writers={writer_slots}, readers={reader_count}, "
            f"killed_writers={killed_writers}, repaired={repaired}, "
            f"hit_samples={len(hit_latencies_ms)}, "
            f"fallback_samples={len(fallback_latencies_ms)}, "
            f"hit_ms_avg={_avg(hit_latencies_ms):.1f}, "
            f"fallback_ms_avg={_avg(fallback_latencies_ms):.1f})."
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

        # Warm may execute a covering/repair subset (python_total) while the plan
        # still lists every commit selector (selected_python). --force remasures
        # the full planned set, so compare forced misses to selected_python.
        py_executed = metric_int(warm["python"].metrics(), "python_total")
        py_planned = metric_int(warm["python"].metrics(), "selected_python")
        rs_selected = metric_int(warm["rust"].metrics(), "rust_final_total")
        assert 0 < py_executed <= py_planned <= py_population
        assert 0 < rs_selected <= rs_population
        assert metric_int(warm["python"].metrics(), "python_cache_hits") == py_executed
        assert metric_int(warm["rust"].metrics(), "rust_final_cache_hits") == rs_selected
        assert metric_int(forced["python"].metrics(), "python_cache_misses") == py_planned
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


@cli.command("timing-rust-throughput")
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
def timing_rust_throughput(
    runs: int,
    job_values: tuple[int, ...],
    legacy_cold_j1_median: float | None,
) -> None:
    """Timing: Rust coverage throughput and external process-tree bounds."""
    assert runs > 0, runs
    assert job_values, "at least one --jobs value is required"
    assert all(jobs > 0 for jobs in job_values), job_values
    samples: list[ThroughputSample] = []
    with qa_fixture("kiss-qa-timing-rust-throughput-") as fixture:
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
                    f"timing-rust-throughput-cold-{sample_index + 1}-j{jobs}",
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
                    f"timing-rust-throughput-warm-{sample_index + 1}-j{jobs}",
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
            "QA PASS: timing-rust-throughput met the legacy cold -j1 acceptance threshold."
        )
    else:
        click.echo(
            "QA PASS: timing-rust-throughput medians and process-tree bounds recorded."
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
        py_gen_manifest = load_json(
            pinned_python_generation_dir(py_cache) / "manifest.json"
        )
        py_source_root = Path(py_gen_manifest["plan"]["base_identity"]["source_root"])
        assert py_source_root.resolve() == fixture.root.resolve()
        py_index = load_python_generation_line_index(py_cache)
        py_index["source_root"] = str(py_source_root)
        py_manifest = load_python_generation_population(py_cache)
        rs_index = load_json(rs_cache / "index.json")
        rs_manifest = load_json(rs_cache / "population.json")
        for payload in (rs_index, rs_manifest):
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
        # Concurrent peers may finish publishing before a late starter plans, so
        # population_required can be false with cache hits. Require at least one
        # true cold populate per language; allow hit-followers.
        py_cold_metrics = [outcome.metrics() for outcome in cold[:3]]
        assert any(
            metrics.get("python_population_required") == "true" for metrics in py_cold_metrics
        ), py_cold_metrics
        for outcome, metrics in zip(cold[:3], py_cold_metrics, strict=True):
            required = metrics.get("python_population_required")
            assert required in {"true", "false"}, (outcome.name, metrics)
            if required == "false":
                assert metric_int(metrics, "python_cache_hits") > 0, (outcome.name, metrics)
        rust_cold_metrics = [outcome.metrics() for outcome in cold[3:]]
        assert any(
            metrics.get("rust_population_required") == "true" for metrics in rust_cold_metrics
        ), rust_cold_metrics
        for outcome, metrics in zip(cold[3:], rust_cold_metrics, strict=True):
            required = metrics.get("rust_population_required")
            assert required in {"true", "false"}, (outcome.name, metrics)
            if required == "false":
                assert (
                    metric_int(metrics, "rust_population_cache_hits")
                    + metric_int(metrics, "rust_final_cache_hits")
                    > 0
                ), (outcome.name, metrics)
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

        # Clear reverse/index/aggregate tokens, but keep population.json so planning
        # stays selective. Then --force the edited Rust source only: commit-mode
        # selection can include logical ids that lack PATH::symbol report ids, which
        # SelectorEntries rejects; RS_SOURCE targets are report-id safe and still
        # activate reverse_line_index for the oracle races below.
        rs_cache = fixture.root / ".kiss/rust_llvm_cov_cache"
        (rs_cache / "index.json").unlink(missing_ok=True)
        (rs_cache / "entry_state.json").unlink(missing_ok=True)
        (rs_cache / "check_aggregate.json").unlink(missing_ok=True)
        shutil.rmtree(rs_cache / "reverse_line_index", ignore_errors=True)
        reverse_prime = run(
            "rust-reverse-prime",
            [
                str(KISS),
                "--lang",
                "rust",
                "test",
                RS_SOURCE.as_posix(),
                "--force",
                "--metrics",
                "-j",
                str(jobs),
            ],
            fixture.root,
            fixture.env,
        )
        assert reverse_prime.returncode == 0, reverse_prime.stderr
        assert_metric(reverse_prime.metrics(), "rust_population_required", "false")
        population = load_json(rs_cache / "population.json")
        assert population.get("reverse_line_index") is not None, population
        assert_rust_reverse_cache_integrity(fixture.root)

        # Warm file + PATH::symbol dry-run readers vs forward-entry oracle.
        rel = RS_SOURCE.as_posix()
        symbol = f"{rel}::format_unreferenced_unit_coverage_message"
        file_readers: list[tuple[list[str], Path]] = []
        symbol_readers: list[tuple[list[str], Path]] = []
        for cwd in (fixture.root, fixture.nested, fixture.root, fixture.nested):
            file_readers.append(
                (
                    [
                        str(KISS),
                        "--defaults",
                        "--lang",
                        "rust",
                        "test",
                        rel,
                        "--dry-run",
                        "--metrics",
                        "-j",
                        str(jobs),
                    ],
                    cwd,
                )
            )
            symbol_readers.append(
                (
                    [
                        str(KISS),
                        "--defaults",
                        "--lang",
                        "rust",
                        "test",
                        symbol,
                        "--dry-run",
                        "--metrics",
                        "-j",
                        str(jobs),
                    ],
                    cwd,
                )
            )
        file_dry = run_concurrent("warm-rust-file-oracle-race", file_readers, fixture.env)
        symbol_dry = run_concurrent(
            "warm-rust-symbol-oracle-race", symbol_readers, fixture.env
        )
        oracle = rust_forward_entry_oracle_selectors(fixture.root, rel)
        assert oracle, f"forward oracle empty for {rel}"
        for outcome in file_dry:
            assert_rust_dry_run_matches_oracle("file", outcome, oracle)
        for outcome in symbol_dry:
            assert_rust_dry_run_matches_oracle(
                "symbol", outcome, oracle, allow_subset=True
            )
        assert len({rendered_plan(outcome) for outcome in file_dry}) == 1, "file"
        assert len({rendered_plan(outcome) for outcome in symbol_dry}) == 1, "symbol"

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

        for language, corrupt_path in (
            # Python rslip v2: population.json is the generation pointer (no index.json).
            ("python", py_cache / "population.json"),
            ("rust", rs_cache / "index.json"),
        ):
            corrupt_path.write_text("{ deliberately broken")
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
            json.loads(corrupt_path.read_text())
            if language == "python":
                pinned_python_generation_dir(py_cache)

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
            "__coverage",
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
        # Deleting only check_aggregate.json leaves selector/population caches warm, so
        # `kiss cov` can skip refresh. Wipe the Rust coverage cache (and records seal)
        # so concurrent callers contend on a real aggregate rebuild.
        reset_rust_check_aggregate_outputs(fixture.root)
        (fixture.root / ".kiss" / "cov_records_cache.json").unlink(missing_ok=True)
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
        click.echo(
            "QA PASS: Rust check aggregate cold publication, warm reuse, "
            "identity repair, code repair, retained maps, and concurrent refresh held."
        )


@cli.command("timing-aggregate-parallel")
def timing_aggregate_parallel() -> None:
    """Timing: parallel −j4 aggregate coverage median < 70% of serial −j1."""
    assert_aggregate_parallel_benchmark()
    click.echo("QA PASS: timing-aggregate-parallel held.")


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
    # Phase-aware SIGINT: wall-clock delays miss warm --force populations that
    # finish the test phase before a fixed timer (historically ~7s < 12s).
    phases = ("build", "test", "export")
    with qa_fixture("kiss-qa-rust-phase-interrupt-") as fixture, log_path.open("w") as log:
        # Widen shim/delegated lifetime so warm --force test/export samples
        # can observe SelectorEntries execution (production leaves this unset).
        fixture.env["KISS_RUST_LLVM_COV_HOLD_BEFORE_GO_MS"] = "750"
        population_command = kiss_command(
            "rust",
            fixture.ignores["rust"],
            "--metrics",
            "--force",
            "-j",
            str(jobs),
        )
        for phase in phases:
            interrupted = run_interrupt_on_phase(
                f"rust-phase-interrupt-{phase}",
                population_command,
                fixture.root,
                fixture.env,
                target_phase=phase,
                repo_root=fixture.root,
            )
            log.write(
                f"{phase}: rc={interrupted.returncode} elapsed={interrupted.elapsed:.2f}s "
                f"target_phase={phase}\n"
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


@cli.command("timing-rust-legacy-warm-baseline")
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
def timing_rust_legacy_warm_baseline(
    batch_warm_median: float, log_dir: Path | None
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
    click.echo(f"Using archived legacy warm baseline: {log_path}")
    click.echo(
        f"QA PASS: timing-rust-legacy-warm-baseline "
        f"batch warm median {batch_warm_median:.2f}s within 10% of "
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
        entry_listings_by_jobs: dict[int, dict[str, int]] = {}
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
            entries_dir = rust_cache / "entries"
            listing: dict[str, int] = {}
            if entries_dir.is_dir():
                for path in sorted(entries_dir.rglob("*")):
                    if path.is_file():
                        listing[str(path.relative_to(entries_dir))] = path.stat().st_size
            entry_listings_by_jobs[jobs] = listing
            if jobs == 1:
                j1_side = Path("/tmp/kiss_qa_retained_j1_entries")
                if j1_side.exists():
                    shutil.rmtree(j1_side)
                shutil.copytree(entries_dir, j1_side)
            tmp_count = sum(1 for name in listing if name.endswith(".tmp"))
            json_bytes = sum(
                size for name, size in listing.items() if name.endswith(".json")
            )
            lines.append(
                f"jobs={jobs} rust_entry_cache_bytes={cache_bytes_by_jobs[jobs]} "
                f"rust_entry_generation_count="
                f"{metric_int(metrics, 'rust_entry_generation_count')} "
                f"entry_files={len(listing)} json_bytes={json_bytes} tmp_files={tmp_count} "
                f"rust_final_cache_misses="
                f"{metric_int(metrics, 'rust_final_cache_misses')} "
                f"rust_final_cache_hits="
                f"{metric_int(metrics, 'rust_final_cache_hits')}"
            )
        if cache_bytes_by_jobs[1] != cache_bytes_by_jobs[4]:
            before = entry_listings_by_jobs[1]
            after = entry_listings_by_jobs[4]
            only_after = sorted(set(after) - set(before))
            only_before = sorted(set(before) - set(after))
            grown = sorted(
                (
                    name,
                    before[name],
                    after[name],
                    after[name] - before[name],
                )
                for name in set(before) & set(after)
                if after[name] != before[name]
            )
            dump_root = Path("/tmp/kiss_qa_retained_entry_diff")
            if dump_root.exists():
                shutil.rmtree(dump_root)
            dump_root.mkdir(parents=True)
            j1_dump = dump_root / "j1"
            j4_dump = dump_root / "j4"
            shutil.copytree(rust_cache / "entries", j4_dump)
            # j1 snapshot was taken into entry_listings only; recover bodies from
            # side copies written after each jobs loop below when present.
            j1_side = Path("/tmp/kiss_qa_retained_j1_entries")
            if j1_side.is_dir():
                shutil.copytree(j1_side, j1_dump)
            coverage_diffs: list[str] = []
            if j1_dump.is_dir():
                for name, _, _, delta in grown[:10]:
                    p1 = j1_dump / name
                    p4 = j4_dump / name
                    if not (p1.is_file() and p4.is_file()):
                        continue
                    e1 = json.loads(p1.read_text())
                    e4 = json.loads(p4.read_text())
                    files1 = {
                        path: sorted(lines)
                        for path, lines in e1.get("coverage", {}).get("files", {}).items()
                    }
                    files4 = {
                        path: sorted(lines)
                        for path, lines in e4.get("coverage", {}).get("files", {}).items()
                    }
                    only_files_4 = sorted(set(files4) - set(files1))
                    only_files_1 = sorted(set(files1) - set(files4))
                    line_delta = sum(len(files4[p]) for p in files4) - sum(
                        len(files1[p]) for p in files1
                    )
                    coverage_diffs.append(
                        f"{name} selector={e4.get('selector')!r} size_delta={delta} "
                        f"line_delta={line_delta} only_files_j4={only_files_4[:5]} "
                        f"only_files_j1={only_files_1[:5]}"
                    )
            raise AssertionError(
                f"cache bytes grew with jobs: {cache_bytes_by_jobs}; "
                f"only_after={only_after[:20]}; only_before={only_before[:20]}; "
                f"grown={grown[:30]}; coverage_diffs={coverage_diffs}; "
                f"dump={dump_root}; lines={lines}"
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
        # Widen the simultaneous nextest/shim/delegated window for observation.
        # Production paths leave this unset (zero hold). Larger than the phase-
        # interrupt hold: distinct-groups must observe three roles at once.
        fixture.env["KISS_RUST_LLVM_COV_HOLD_BEFORE_GO_MS"] = "2000"
        population_command = kiss_command(
            "rust",
            fixture.ignores["rust"],
            "--metrics",
            "--force",
            "-j",
            str(jobs),
        )
        # Cold compile can finish before the observer arms; warm the batch first
        # (same pattern as rust-phase-interrupt) so the interrupt run spends its
        # wall time in SelectorEntries with HOLD applied.
        warm = run(
            "rust-distinct-groups-warmup",
            population_command,
            fixture.root,
            fixture.env,
        )
        assert warm.returncode == 0, warm.combined
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


@cli.command("profraw-discard-sink")
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
    first_names = _discard_profraw_names(ROOT)
    assert first_names, f"expected dumps under {discard}"

    _run_kiss_help(ROOT, env)
    second_names = _discard_profraw_names(ROOT)
    assert first_names.isdisjoint(second_names), (
        f"startup sweep did not clear prior discard dumps: "
        f"prior={sorted(first_names)} still={sorted(first_names & second_names)}"
    )
    assert second_names, f"expected fresh dump under {discard} after second --help"

    _unlink_default_profraw(nested)
    _run_kiss_help(nested, env)
    assert not list(ROOT.glob("default_*.profraw")), list(ROOT.glob("default_*.profraw"))
    assert not list(nested.glob("default_*.profraw")), list(nested.glob("default_*.profraw"))
    assert _discard_profraw_names(ROOT), f"expected dumps under {discard} from nested cwd"

    with qa_fixture("kiss-qa-profraw-") as fixture:
        planted = fixture.root / "default_scrubbed_0_424242.profraw"
        planted.write_bytes(b"orphan-from-scrubbed-child")
        batch_env = dict(fixture.env)
        batch_env.pop("LLVM_PROFILE_FILE", None)
        outcome = run(
            "profraw-batch-begin-orphan-sweep",
            kiss_command(
                "rust",
                fixture.ignores["rust"],
                "--metrics",
                "-j",
                "1",
            ),
            fixture.root,
            batch_env,
        )
        assert outcome.returncode == 0, outcome.combined
        assert not planted.exists(), "batch-begin must remove scrubbed-child root dumps"

    click.echo(
        "QA PASS: absolute .kiss/profraw redirect, startup sweep, nested cwd, "
        "and batch-begin orphan cleanup held."
    )


if __name__ == "__main__":
    cli()
