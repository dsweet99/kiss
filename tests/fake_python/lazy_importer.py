def use_helper():
    # Lazy import (inside a function). This should still create a dependency edge
    # to the internal module `tests.fake_python.lazy_target`.
    from tests.fake_python.lazy_target import do_work
    return do_work()
