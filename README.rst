.. image:: https://travis-ci.com/agronholm/cbor2.svg?branch=master
  :target: https://travis-ci.com/agronholm/cbor2
  :alt: Build Status
.. image:: https://coveralls.io/repos/github/agronholm/cbor2/badge.svg?branch=master
  :target: https://coveralls.io/github/agronholm/cbor2?branch=master
  :alt: Code Coverage
.. image:: https://readthedocs.org/projects/cbor2/badge/?version=latest
  :target: https://cbor2.readthedocs.io/en/latest/?badge=latest
  :alt: Documentation Status

This library provides encoding and decoding for the Concise Binary Object Representation (CBOR)
(`RFC 7049`_) serialization format. `Read the docs <https://cbor2.readthedocs.io/>`_ to learn more.

It is implemented in pure python with an optional C backend and is compatible with versions 2.7 through to 3.7.

On cPython>=3.3 cbor2 can use a built in C module for performance similar to how ``pickle``
wraps the ``_pickle`` C module in the Python Standard Library. On Windows, this is restricted to cPython>=3.5.

On PyPy, cbor2 runs with almost identical performance to the C backend.

.. _RFC 7049: https://tools.ietf.org/html/rfc7049
