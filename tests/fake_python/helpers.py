# helpers.py - NOT a test file (no test naming pattern)
# Even though it imports pytest, helper functions are not tests

import pytest


def assert_valid_email(email):
    """Helper used by tests - but this file is NOT a test file."""
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

