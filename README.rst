.. image:: https://travis-ci.org/agronholm/cbor2.svg?branch=master
  :target: https://travis-ci.org/agronholm/cbor2
  :alt: Build Status
.. image:: https://coveralls.io/repos/github/agronholm/cbor2/badge.svg?branch=master
  :target: https://coveralls.io/github/agronholm/cbor2?branch=master
  :alt: Code Coverage
.. image:: https://readthedocs.org/projects/cbor2/badge/?version=latest
  :target: https://cbor2.readthedocs.io/en/latest/?badge=latest
  :alt: Documentation Status

  This library provides encoding and decoding for the Concise Binary Object Representation (CBOR)
(`RFC 7049`_) serialization format.

There exists another Python CBOR implementation (cbor) which is faster on CPython due to its C
extensions. On PyPy, cbor2 and cbor are almost identical in performance. The other implementation
also lacks documentation and a comprehensive test suite, does not support most standard extension
tags and is known to crash (segfault) when passed a cyclic structure (say, a list containing
itself).

.. _RFC 7049: https://tools.ietf.org/html/rfc7049
