def assert_valid_email(email):
    assert "@" in email


def create_mock_response(status=200):
    """Another helper."""
    return {"status": status, "body": None}


class ResponseBuilder:
    """Helper class - NOT a test class (no Test prefix)."""

    def __init__(self):
        self.status = 200

    def with_status(self, status):
        self.status = status
        return self

    def build(self):
        return {"status": self.status}

