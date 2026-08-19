"""CLI shim for coverage metrics (delegates to python.coverage_metrics)."""

from python.coverage_metrics import coverage_metrics_cli as main

if __name__ == "__main__":
    main()
