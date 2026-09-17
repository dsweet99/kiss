#!/usr/bin/env python3
import importlib
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
main = importlib.import_module("python.coverage_metrics").coverage_metrics_cli

if __name__ == "__main__":
    main()
