import sys
import platform
from setuptools import setup, Extension


if platform.python_implementation() == 'CPython' and sys.version_info >= (3, 3):
    _cbor2 = Extension(
        '_cbor2',
        libraries=['m'],
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
