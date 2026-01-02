import sys

import atheris

# _cbor2 ensures the C library is imported
from _cbor2 import loads


def test_one_input(data: bytes):
    try:
        loads(data)
    except Exception:
        # We're searching for memory corruption, not Python exceptions
        pass


if __name__ == "__main__":
    atheris.Setup(sys.argv, test_one_input)
    atheris.Fuzz()
