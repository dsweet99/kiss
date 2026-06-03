"""Shared PATH stubs for coverage_metrics tests."""

from __future__ import annotations

import stat
import textwrap
from pathlib import Path


def write_executable(path: Path, body: str) -> None:
    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def install_path_stub(tmp_path: Path, name: str, script: str) -> Path:
    bindir = tmp_path / "bin"
    bindir.mkdir(parents=True, exist_ok=True)
    write_executable(bindir / name, script)
    return bindir


SLIPCOVER_OK = textwrap.dedent(
    """\
    #!/usr/bin/env python3
    import json, sys
    from pathlib import Path
    out = Path(sys.argv[sys.argv.index("--out") + 1])
    out.write_text(json.dumps({
        "files": {
            "pkg/a.py": {"summary": {"percent_covered": 80.0}},
        }
    }))
    sys.exit(0)
    """
)

CARGO_LLVM_OK = textwrap.dedent(
    """\
    #!/usr/bin/env python3
    import json, sys
    from pathlib import Path
    if "llvm-cov" not in sys.argv:
        sys.exit(1)
    out = Path(sys.argv[sys.argv.index("--output-path") + 1])
    out.write_text(json.dumps({
        "data": [{
            "files": [{
                "filename": "src/lib.rs",
                "summary": {"lines": {"percent": 55.5}},
            }],
        }],
    }))
    sys.exit(0)
    """
)
