from pkg.caller import run


def test_import_forms():
    assert run() == (5, 6, 7)
