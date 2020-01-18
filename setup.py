import sys
import platform
from pkg_resources import parse_version
from setuptools import setup, Extension

cpython = platform.python_implementation() == 'CPython'
is_glibc = platform.libc_ver()[0] == 'glibc'
windows = sys.platform.startswith('win')
if is_glibc:
    glibc_ver = platform.libc_ver()[1]
    libc_ok = parse_version(glibc_ver) >= parse_version('2.9')
else:
    libc_ok = not windows
min_win_version = windows and sys.version_info >= (3, 5)
min_unix_version = not windows and sys.version_info >= (3, 3)

# Enable GNU features for libc's like musl, should have no effect
# on Apple/BSDs
if libc_ok:
    gnu_flag = ['-D_GNU_SOURCE']
else:
    gnu_flag = []

if cpython and ((min_unix_version and libc_ok) or min_win_version):
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
        ]
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
        'setuptools >= 36.2.7',
        'setuptools_scm >= 1.7.0'
    ],
    **kwargs
    )
