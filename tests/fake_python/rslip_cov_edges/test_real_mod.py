"""Trivial pytest that exercises real_mod for live coverage."""

try:
    from .real_mod import marker
except ImportError:
    from tests.fake_python.rslip_cov_edges.real_mod import marker


def test_marker():
    assert marker() == 1
