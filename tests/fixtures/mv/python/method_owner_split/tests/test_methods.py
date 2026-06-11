from pkg.reviewer import Reviewer
from pkg.source import Worker


def test_methods_stay_distinct():
    assert Worker().run(4) == 5
    assert Reviewer().run(4) == 6
