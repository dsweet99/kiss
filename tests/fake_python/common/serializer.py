import json as _j


class JsonDecodeError(ValueError):
    pass


def dumps(obj):
    return _j.dumps(obj)


def loads(s):
    try:
        return _j.loads(s)
    except _j.JSONDecodeError as e:
        raise JsonDecodeError(str(e)) from e


def load_file(path):
    with open(path, encoding="utf-8") as f:
        return loads(f.read())


def dump_file(path, obj, indent=2, default=str):
    with open(path, "w", encoding="utf-8") as f:
        f.write(_j.dumps(obj, indent=indent, default=default))


def jserialize_file(path, obj):
    dump_file(path, obj)
