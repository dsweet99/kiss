# Test file identified by *_test.py naming pattern

try:
    from .api_handler import handle_api_request
except ImportError:
    from tests.fake_python.api_handler import handle_api_request


def make_handler():
    """Helper function - NOT a test."""
    return handle_api_request


def test_handler_init():
    """Test function."""
    handler = make_handler()
    assert handler is not None


def test_handler_process():
    """Test function."""
    result = handle_api_request({})
    assert result is not None


class TestApiHandler:
    """Test class."""

    def test_handle_request(self):
        response = handle_api_request({"method": "GET", "path": "/api/health"})
        assert response["status"] == 200

