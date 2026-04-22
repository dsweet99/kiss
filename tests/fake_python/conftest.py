# conftest.py - pytest fixture file (always identified as test infrastructure)

import pytest

from .models import Product, User


@pytest.fixture
def sample_user():
    """Fixture function - provides test data."""
    return User(1, "fixture", "fixture@example.com")


@pytest.fixture
def sample_product():
    """Another fixture."""
    return Product(1, "Test Product", 29.99, 1)


def create_test_database():
    """Helper function in conftest - NOT a fixture or test."""
    return {"users": [], "products": []}

