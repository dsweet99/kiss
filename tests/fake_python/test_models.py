# Test file identified by test_*.py naming pattern
# Contains pytest conventions: test_ functions and Test classes

from models import User, Product


def helper_create_user():
    """Helper function - NOT a test (no test_ prefix)."""
    return User("test@example.com")


def test_user_creation():
    """Test function identified by test_ prefix."""
    user = helper_create_user()
    assert user.email == "test@example.com"


def test_product_price():
    """Another test function."""
    product = Product("Widget", 9.99)
    assert product.price > 0


class TestUser:
    """Test class identified by Test prefix."""

    def setup_method(self):
        """Setup method - NOT a test but part of test infrastructure."""
        self.user = User("class@example.com")

    def test_email_format(self):
        """Test method identified by test_ prefix."""
        assert "@" in self.user.email

    def test_user_repr(self):
        """Another test method."""
        assert "User" in repr(self.user)

    def helper_validate(self):
        """Helper method - NOT a test (no test_ prefix)."""
        return self.user.email is not None


class TestProduct:
    """Another test class."""

    def test_product_name(self):
        product = Product("Gadget", 19.99)
        assert product.name == "Gadget"

