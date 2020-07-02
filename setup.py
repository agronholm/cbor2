import sys
import os
import platform
from pkg_resources import parse_version
from setuptools import setup, Extension

min_glibc = parse_version('2.9')


def check_libc():
    "check that if we have glibc < 2.9 we should not build c ext"
    # Borrowed from pip internals
    # https://github.com/pypa/pip/blob/20.1.1/src/pip/_internal/utils/glibc.py#L21-L36
    try:
        # os.confstr("CS_GNU_LIBC_VERSION") returns a string like "glibc 2.17":
        libc, version = os.confstr("CS_GNU_LIBC_VERSION").split()
    except (AttributeError, OSError, ValueError):
        # os.confstr() or CS_GNU_LIBC_VERSION not available (or a bad value)...
        return True
    if libc != 'glibc':
        # Attempt to build with musl or other libc
        return True
    return parse_version(version) >= min_glibc


cpython = platform.python_implementation() == 'CPython'
windows = sys.platform.startswith('win')
min_win_version = sys.version_info >= (3, 5)
min_unix_version = sys.version_info >= (3, 3)
build_c_ext = cpython and ((windows and min_win_version) or (check_libc() and min_unix_version))

# Enable GNU features for libc's like musl, should have no effect
# on Apple/BSDs
if build_c_ext and not windows:
    gnu_flag = ['-D_GNU_SOURCE']
else:
    gnu_flag = []

if build_c_ext:
    _cbor2 = Extension(
        '_cbor2',
        # math.h routines are built-in to MSVCRT
        libraries=['m'] if not windows else [],
        extra_compile_args=['-std=c99'] + gnu_flag,
        sources=[
            'source/module.c',
            'source/encoder.c',
            'source/decoder.c',
            'source/tags.c',
            'source/halffloat.c',
        ],
        optional=True
    )
    kwargs = {'ext_modules': [_cbor2]}
else:
    kwargs = {}


setup(
    use_scm_version={
        'version_scheme': 'post-release',
        'local_scheme': 'dirty-tag'
    },
    setup_requires=[
        'setuptools >= 40.7.0',
        'setuptools_scm >= 1.7.0'
    ],
    **kwargs
    )
