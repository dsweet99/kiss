# Test file identified by *_test.py naming pattern

try:
    from .api_handler import handle_api_request
except ImportError:
    from tests.fake_python.api_handler import handle_api_request


def make_handler():
    return handle_api_request


def test_handler_init():
    handler = make_handler()
    assert handler is not None


def test_handler_process():
    result = handle_api_request({})
    assert result is not None


class TestApiHandler:

    def test_handle_request(self):
        response = handle_api_request({"method": "GET", "path": "/api/health"})
        assert response["status"] == 200

