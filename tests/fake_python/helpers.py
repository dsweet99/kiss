def assert_valid_email(email):
    assert "@" in email


def create_mock_response(status=200):
    return {"status": status, "body": None}


class ResponseBuilder:

    def __init__(self):
        self.status = 200

    def with_status(self, status):
        self.status = status
        return self

    def build(self):
        return {"status": self.status}

