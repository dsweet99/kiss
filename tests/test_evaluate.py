from __future__ import annotations

import python
from ops.evaluate import evaluation_names, main, run_evaluation


def test_evaluation_names_include_qa_commands() -> None:
    assert python.__name__ == "python"
    names = evaluation_names()
    assert "coverage_cache_witness" in names
    assert "timing_rust_throughput" in names
    assert "kiss_test_watch" in names
    assert all(" " not in name for name in names)


def test_evaluate_list_prints_names(capsys) -> None:
    main(["list"])
    printed = capsys.readouterr().out.splitlines()
    assert printed == evaluation_names()


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
    run_evaluation("coverage_cache_witness")
    assert called == ["ran"]
