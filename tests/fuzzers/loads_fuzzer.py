import sys

import atheris
from cbor2 import loads


def test_one_input(data: bytes) -> None:
    try:
        loads(data)
    except Exception:
        # We're searching for memory corruption, not Python exceptions
        pass


if __name__ == "__main__":
    atheris.Setup(sys.argv, test_one_input)
    atheris.Fuzz()
