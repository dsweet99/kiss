"""Well-written utility functions - example of clean code."""


def calculate_average(numbers: list[float]) -> float:
    """Calculate the average of a list of numbers."""
    if not numbers:
        return 0.0
    return sum(numbers) / len(numbers)


def clamp(value: float, min_val: float, max_val: float) -> float:
    """Clamp a value between min and max bounds."""
    return max(min_val, min(value, max_val))


def is_valid_email(email: str) -> bool:
    """Basic email validation."""
    if not email or "@" not in email:
        return False
    local, domain = email.rsplit("@", 1)
    return bool(local) and "." in domain


class Counter:
    """A simple counter with increment/decrement."""

    def __init__(self, initial: int = 0):
        self._value = initial

    def increment(self) -> int:
        self._value += 1
        return self._value

    def decrement(self) -> int:
        self._value -= 1
        return self._value

    @property
    def value(self) -> int:
        return self._value

