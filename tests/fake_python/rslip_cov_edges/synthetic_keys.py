"""Canonical non-file coverage key shapes for rslip digest edge cases.

Rust tests parse these assigned string literals (include_str!) so shapes cannot
drift from this fixture.
"""

TYPE_ABSOLUTE = "/placeholder/src/[type CamusGateway_ABCDEF01]"
TYPE_RELATIVE = "[type RelativeGateway_12345678]"
FROZEN = "<frozen importlib._bootstrap>"
RSLIP_RUNTIME = "/tmp/workspace/.kiss/rslip_runtime/plugin.py"
KISS_RUNTIME = ".kiss/rslip_runtime/hook.py"
