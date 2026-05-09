def helper(value: int) -> int:
    label = "helper should stay in strings"
    _ = label
    return value + 1


def untouched_helper() -> int:
    return 10
