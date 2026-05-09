def use_helper():
    from tests.fake_python.lazy_target import do_work

    total = 0
    for _ in range(1):
        total += do_work()
    return total
