"""Fixture: internal module with external imports."""

import json
import os

from tests.fake_python import graph_ext_b

_ = graph_ext_b

__all__ = ["os", "json", "graph_ext_b"]

