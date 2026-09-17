from __future__ import annotations

import os
from pathlib import Path

import python
from evals._harness import emit_eval, report_eval, run_concurrent
from ops.evaluate import evaluation_names, main, run_evaluation


def test_evaluation_names_include_qa_commands() -> None:
    assert python.__name__ == "python"
    names = evaluation_names()
    assert "short/coverage_cache_witness" in names
    assert "short/timing_rust_throughput" in names
    assert "short/timing_kiss_check" in names
    assert "short/timing_kiss_test" in names
    assert "short/timing_kiss_test_watch" in names
    assert "short/timing_kiss_test_watch_cache_hit" in names
    assert "short/timing_kiss_test_sigint_restart" in names
    assert "short/timing_kiss_test_sigint_work_reuse" in names
    assert "short/timing_kiss_test_retry_bad" in names
    assert "short/timing_kiss_test_watch_dirty" in names
    assert "short/timing_kiss_test_progress_cpu" in names
    assert "long/timing_kiss_test" in names
    assert "long/timing_kiss_test_sympy" in names
    assert "long/timing_kiss_test_anydoc" in names
    assert "short/kiss_test_watch" not in names
    assert all(" " not in name for name in names)
    assert all(not name.split("/", 1)[-1].startswith("kiss_test_") for name in names)


def test_evaluate_list_and_run_all(capsys, monkeypatch) -> None:
    main(["list"])
    printed = capsys.readouterr().out.splitlines()
    assert printed == evaluation_names()
    called: list[str] = []
    monkeypatch.setattr(
        "ops.evaluate._run_evaluation_subprocess",
        lambda name: called.append(name) or 0,
    )
    main(["run-all"])
    assert called == evaluation_names()

    names = evaluation_names()
    assert len(names) >= 2
    monkeypatch.setattr(
        "ops.evaluate._run_evaluation_subprocess",
        lambda name: 1 if name == names[0] else 0,
    )
    try:
        main(["run-all"])
    except SystemExit as exc:
        assert exc.code == 1
    else:
        raise AssertionError("expected SystemExit(1) when an eval fails")
    out = capsys.readouterr().out
    assert f"FAILED: {names[0]}" in out
    assert f"run-all: 1 failed: {names[0]}" in out


def test_evaluate_run_unknown_exits() -> None:
    try:
        main(["run", "not_a_real_eval"])
    except SystemExit as exc:
        assert "unknown evaluation" in str(exc)
    else:
        raise AssertionError("expected SystemExit")


def test_run_evaluation_invokes_wrapper(monkeypatch) -> None:
    called: list[str] = []

    class Fake:
        @staticmethod
        def eval_coverage_cache_witness() -> None:
            called.append("ran")

    monkeypatch.setattr("importlib.import_module", lambda name: Fake())
    run_evaluation("short/coverage_cache_witness")
    assert called == ["ran"]
    run_evaluation("coverage_cache_witness")
    assert called == ["ran", "ran"]


def test_emit_eval_rejects_pass_fail_kinds() -> None:
    try:
        emit_eval("held", "PASS")
    except ValueError as exc:
        assert "LARGER/SMALLER" in str(exc)
    else:
        raise AssertionError("expected ValueError for PASS")
    try:
        emit_eval("held", "FAIL")
    except ValueError as exc:
        assert "LARGER/SMALLER" in str(exc)
    else:
        raise AssertionError("expected ValueError for FAIL")


def test_report_eval_does_not_determine_or_print_pass_fail(capsys) -> None:
    def missed_property() -> None:
        raise AssertionError("measured property did not hold")

    report_eval(missed_property)
    out = capsys.readouterr().out
    lines = [line for line in out.splitlines() if line.startswith("EVAL:")]
    assert lines[0].startswith("EVAL: elapsed_s = SMALLER(")
    assert lines[1].startswith("EVAL: peak_rss_kib = SMALLER(")
    assert all("PASS" not in line and "FAIL" not in line for line in out.splitlines())


def test_run_concurrent_does_not_print_child_pass_fail(capsys) -> None:
    def body() -> None:
        run_concurrent(
            "echo-fail",
            [
                (
                    [
                        "python3",
                        "-c",
                        "print('FAIL: example'); print('PASS: example'); raise SystemExit(1)",
                    ],
                    Path("."),
                )
            ],
            env=os.environ.copy(),
            timeout=10,
        )

    report_eval(body)
    out = capsys.readouterr().out
    assert "FAIL: example" not in out
    assert "PASS: example" not in out
    assert "EVAL: elapsed_s = SMALLER(" in out
