# Test file identified by test_*.py naming pattern
# Contains pytest conventions: test_ functions and Test classes

from models import Product, User


def helper_create_user():
    """Helper function - NOT a test (no test_ prefix)."""
    return User(1, "tester", "test@example.com")


def test_user_creation():
    """Test function identified by test_ prefix."""
    user = helper_create_user()
    assert user.email == "test@example.com"


def test_product_price():
    """Another test function."""
    product = Product(1, "Widget", 9.99, 1)
    assert product.price > 0


def test_email_format():
    user = User(2, "classuser", "class@example.com")
    assert "@" in user.email


def test_user_repr():
    user = User(2, "classuser", "class@example.com")
    assert "User" in repr(user)


def test_product_name():
    product = Product(2, "Gadget", 19.99, 1)
    assert product.name == "Gadget"
