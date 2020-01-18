import sys
import platform

import pytest

import cbor2.types
import cbor2.encoder
import cbor2.decoder

load_exc = ''
try:
    import _cbor2
except ImportError as e:
    if not str(e).startswith('No module'):
        load_exc = str(e)
    _cbor2 = None

cpython33 = pytest.mark.skipif(
    platform.python_implementation() != "CPython"
    or sys.version_info < (3, 3)
    or _cbor2 is None,
    reason=(load_exc or "requires CPython 3.3+"),
)


class Module(object):
    # Mock module class
    pass


@pytest.fixture(params=[pytest.param("c", marks=cpython33), "python"], scope="session")
def impl(request):
    if request.param == "c":
        return _cbor2
    else:
        # Make a mock module of cbor2 which always contains the pure Python
        # implementations, even if the top-level package has imported the
        # _cbor2 module
        module = Module()
        for source in (cbor2.types, cbor2.encoder, cbor2.decoder):
            for name in dir(source):
                setattr(module, name, getattr(source, name))
        return module
