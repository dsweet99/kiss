import time


def now_isoformat() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime())
