.. image:: https://travis-ci.com/agronholm/cbor2.svg?branch=master
  :target: https://travis-ci.com/agronholm/cbor2
  :alt: Build Status
.. image:: https://coveralls.io/repos/github/agronholm/cbor2/badge.svg?branch=master
  :target: https://coveralls.io/github/agronholm/cbor2?branch=master
  :alt: Code Coverage
.. image:: https://readthedocs.org/projects/cbor2/badge/?version=latest
  :target: https://cbor2.readthedocs.io/en/latest/?badge=latest
  :alt: Documentation Status

About
=====

This library provides encoding and decoding for the Concise Binary Object Representation (CBOR)
(`RFC 7049`_) serialization format. `Read the docs <https://cbor2.readthedocs.io/>`_ to learn more.

It is implemented in pure python with an optional C backend.

On PyPy, cbor2 runs with almost identical performance to the C backend.

.. _RFC 7049: https://tools.ietf.org/html/rfc7049

Features
--------

* Simple api like ``json`` or ``pickle`` modules.
* Support many `CBOR tags`_ with `stdlib objects`_.
* Generic tag decoding.
* `Shared value`_ references including cyclic references.
* Optional C module backend.
* Extensible `tagged value handling`_ using ``tag_hook`` and ``object_hook`` on decode and ``default`` on encode.
* Command-line diagnostic tool, converting CBOR file or stream to JSON ``python -m cbor2.tool``
  (This is a lossy conversion, for diagnostics only)

.. _CBOR tags: https://www.iana.org/assignments/cbor-tags/cbor-tags.xhtml
.. _stdlib objects: https://cbor2.readthedocs.io/en/latest/usage.html#tag-support
.. _Shared value: http://cbor.schmorp.de/value-sharing
.. _tagged value handling: https://cbor2.readthedocs.io/en/latest/customizing.html#using-the-cbor-tags-for-custom-types

Installation
============

::
    pip install cbor2

Requirements
------------

* cPython==2.7 or cPython>=3.5 or `PyPy`_ (Python 2.7 support will be removed soon)
* C-extension: Any C compiler that can build Python extensions.
  Any modern libc with the exception of Glibc<2.9

.. _PyPy: https://www.pypy.org/

Building the C-Extension
------------------------

To force building of the optional C-extension, set OS env ``CBOR2_BUILD_C_EXTENSION=1``.
To disable building of the optional C-extension, set OS env ``CBOR2_BUILD_C_EXTENSION=0``.
If this environment variable is unset, setup.py will default to auto detecting a compatible C library and
attempt to compile the extension.


Usage
=====

`Basic Usage <https://cbor2.readthedocs.io/en/latest/usage.html#basic-usage>`_

Command-line Usage
==================

.. include:: cbor2/tool.py
   :start-line: 8
   :end-line: 20
   :literal:
