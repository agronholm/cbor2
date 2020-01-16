import sys
import platform

import pytest

import cbor2.types
import cbor2.encoder
import cbor2.decoder

try:
    import _cbor2
except ImportError as e:
    _cbor2 = None

is_glibc = platform.libc_ver()[0] == 'glibc'
glibc_old = is_glibc and platform.libc_ver()[1] < '2.9'

cpython33 = pytest.mark.skipif(
    platform.python_implementation() != 'CPython' or sys.version_info < (3, 3) or glibc_old,
    reason="requires CPython 3.3+ and glibc 2.9+")


class Module(object):
    # Mock module class
    pass


@pytest.fixture(params=[
    pytest.param('c', marks=cpython33),
    'python'
], scope='session')
def impl(request):
    if request.param == 'c':
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
