# Test file identified by *_test.py naming pattern

from api_handler import ApiHandler


def make_handler():
    """Helper function - NOT a test."""
    return ApiHandler()


def test_handler_init():
    """Test function."""
    handler = make_handler()
    assert handler is not None


def test_handler_process():
    """Test function."""
    handler = ApiHandler()
    result = handler.process({})
    assert result is not None


def test_handle_request():
    handler = ApiHandler()
    response = handler.handle_request("/api/test")
    assert response.status == 200
