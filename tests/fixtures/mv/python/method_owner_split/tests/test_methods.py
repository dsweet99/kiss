from pkg.source import Reviewer, Worker


def test_methods_stay_distinct():
    assert Worker().run(4) == 5
    assert Reviewer().run(4) == 6
