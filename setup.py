import sys
import platform
from setuptools import setup, Extension

cpython = platform.python_implementation() == 'CPython'
windows = sys.platform.startswith('win')
min_win_version = windows and sys.version_info >= (3, 5)
min_unix_version = not windows and sys.version_info >= (3, 3)

if cpython and (min_unix_version or min_win_version):
    _cbor2 = Extension(
        '_cbor2',
        # math.h routines are built-in to MSVCRT
        libraries=['m'] if not windows else [],
        extra_compile_args=['-std=c99'],
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
