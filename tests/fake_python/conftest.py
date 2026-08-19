# conftest.py - pytest fixture file (always identified as test infrastructure)

import pytest

try:
    from .models import Product, User
except ImportError:
    from tests.fake_python.models import Product, User


@pytest.fixture
def sample_user():
    return User(1, "fixture_user", "fixture@example.com")


@pytest.fixture
def sample_product():
    return Product(1, "Test Product", 29.99, 1)


def create_test_database():
    return {"users": [], "products": []}

