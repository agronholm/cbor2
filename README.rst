.. image:: https://github.com/agronholm/cbor2/actions/workflows/test.yml/badge.svg
  :target: https://github.com/agronholm/cbor2/actions/workflows/test.yml
  :alt: Testing Status
.. image:: https://github.com/agronholm/cbor2/actions/workflows/publish.yml/badge.svg
  :target: https://github.com/agronholm/cbor2/actions/workflows/publish.yml
  :alt: Publish Status
.. image:: https://coveralls.io/repos/github/agronholm/cbor2/badge.svg?branch=master
  :target: https://coveralls.io/github/agronholm/cbor2?branch=master
  :alt: Code Coverage
.. image:: https://readthedocs.org/projects/cbor2/badge/?version=latest
  :target: https://cbor2.readthedocs.io/en/latest/?badge=latest
  :alt: Documentation Status
.. image:: https://tidelift.com/badges/package/pypi/cbor2
  :target: https://tidelift.com/subscription/pkg/pypi-cbor2
  :alt: Tidelift

About
=====

This library provides encoding and decoding for the Concise Binary Object Representation (CBOR)
(`RFC 8949`_) serialization format. The specification is fully compatible with the original RFC
7049. `Read the docs <https://cbor2.readthedocs.io/>`_ to learn more.

It is implemented in Rust.

.. _RFC 8949: https://www.rfc-editor.org/rfc/rfc8949.html

Features
--------

* Simple API like the ``json`` or ``pickle`` modules
* Support many `CBOR tags`_ with `stdlib objects`_
* Generic tag decoding
* `Shared value`_ references including cyclic references
* `String references`_ compact encoding with repeated strings replaced with indices
* Extensible `tagged value handling`_ using ``tag_hook`` and ``object_hook`` on decode and
  ``default`` on encode.
* Command-line diagnostic tool, converting CBOR file or stream to JSON ``python -m cbor2.tool``
  (This is a lossy conversion, for diagnostics only)
* Thorough test suite (Tested on big- and little-endian architectures)

.. _CBOR tags: https://www.iana.org/assignments/cbor-tags/cbor-tags.xhtml
.. _stdlib objects: https://cbor2.readthedocs.io/en/latest/usage.html#tag-support
.. _Shared value: http://cbor.schmorp.de/value-sharing
.. _String references: http://cbor.schmorp.de/stringref
.. _tagged value handling: https://cbor2.readthedocs.io/en/latest/customizing.html#using-the-cbor-tags-for-custom-types

Installation
============

The simplest way to install the library is with pip_:

::

    pip install cbor2

If this fails, see the next section.

.. _pip: https://packaging.python.org/en/latest/tutorials/installing-packages/

Build requirements
------------------

If you wish to compile the code yourself, or are installing on a yet unsupported Python version
or platform where there are no wheels available, you need the following pre-requisites:

* Python >= 3.10 (or `PyPy3`_ 3.11+)
* `Rust toolchain`_ (tested with v1.93.0)

.. _PyPy3: https://www.pypy.org/
.. _Rust toolchain: https://rust-lang.org/tools/install/

Usage
=====

`Basic Usage <https://cbor2.readthedocs.io/en/latest/usage.html#basic-usage>`_

Command-line Usage
==================

The provided command line tool (``cbor2``) converts CBOR data in raw binary or base64
encoding into a representation that allows printing as JSON. This is a lossy
transformation as each datatype is converted into something that can be represented as a
JSON value.

The tool can alternatively be invoked with ``python -m cbor2.tool``.

Usage::

    # Pass hexadecimal through xxd.
    $ echo a16568656c6c6f65776f726c64 | xxd -r -ps | cbor2 --pretty
    {
        "hello": "world"
    }
    # Decode Base64 directly
    $ echo ggEC | python -m cbor2.tool --decode
    [1, 2]
    # Read from a file encoded in Base64
    $ python -m cbor2.tool -d tests/examples.cbor.b64
    {...}

It can be used in a pipeline with json processing tools like `jq`_ to allow syntax
coloring, field extraction and more.

CBOR data items concatenated into a sequence can be decoded also::

    $ echo ggECggMEggUG | cbor2 -d --sequence
    [1, 2]
    [3, 4]
    [5, 6]

Multiple files can also be sent to a single output file::

    $ cbor2 -o all_files.json file1.cbor file2.cbor ... fileN.cbor

.. _jq: https://stedolan.github.io/jq/

Security
========

This library has not been tested against malicious input. In theory it should be
as safe as JSON, since unlike ``pickle`` the decoder does not execute any code.
