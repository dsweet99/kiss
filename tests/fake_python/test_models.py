# Test file identified by test_*.py naming pattern
# Contains pytest conventions: test_ functions and Test classes

try:
    from .models import Product, User
except ImportError:
    from tests.fake_python.models import Product, User



def helper_create_user():
    return User(1, "tester", "test@example.com")


def test_user_creation():
    user = helper_create_user()
    assert user.email == "test@example.com"


def test_product_price():
    product = Product(1, "Widget", 9.99, 1)
    assert product.price > 0


def test_email_format():
    user = User(2, "classuser", "class@example.com")
    assert "@" in user.email

class TestUser:

    def setup_method(self):
        self.user = User(2, "classuser", "class@example.com")

    def test_email_format(self):
        assert "@" in self.user.email

    def test_user_repr(self):
        assert "User" in repr(self.user)

    def helper_validate(self):
        return self.user.email is not None

def test_user_repr():
    user = User(2, "classuser", "class@example.com")
    assert "User" in repr(user)

def test_product_name():
    product = Product(2, "Gadget", 19.99, 1)
    assert product.name == "Gadget"
