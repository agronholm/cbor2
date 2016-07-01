.. image:: https://travis-ci.org/agronholm/cbor2.svg?branch=master
  :target: https://travis-ci.org/agronholm/cbor2
  :alt: Build Status
.. image:: https://coveralls.io/repos/github/agronholm/cbor2/badge.svg?branch=master
  :target: https://coveralls.io/github/agronholm/cbor2?branch=master
  :alt: Code Coverage

This library provides encoding and decoding for the Concise Binary Object Representation (CBOR)
(`RFC 7049`_) serialization format.

Usage
-----

.. code-block:: python

  from cbor2 import *

  # Serialize an object as a bytestring
  data = dumps(['hello', 'world'])

  # Deserialize a bytestring
  obj = loads(data)

  # Efficiently deserialize from a file
  with open('input.cbor', 'rb') as fp:
      obj = load(fp)

  # Efficiently serialize an object to a file
  with open('output.cbor', 'wb') as fp:
      dump(obj, fp)

  # Iteratively decode a datastream
  for chunk in CBORDecoder().decode(data):
    ...

String/bytes handling on Python 2
---------------------------------

Bytestrings are encoded as binary strings on Python 2. If you want to encode strings as text on
Python 2, use unicode strings instead.

Date/time handling
------------------

CBOR does not support naïve datetimes (that is, datetimes where ``tzinfo`` is missing).
When the encoder encounters such a datetime, it needs to know which timezone it belongs to.
To this end, you can specify a default timezone by passing a ``datetime.tzinfo`` instance to
``dump()``/``dumps()`` call as the ``timezone`` argument.
Decoded datetimes are always timezone aware.

By default, datetimes are serialized in a manner that retains their timezone offsets. You can
optimize the data stream size by passing ``datetime_as_timestamp=False`` to ``dump()``/``dumps()``,
but this causes the timezone offset information to be lost.

Cyclic (recursive) data structures
----------------------------------

By default, both the encoder and decoder support cyclic data structures (ie. containers that
contain references to themselves). When serializing, this requires some extra space in the data
stream. If you know you won't have cyclic structures in your data, you can turn off the value
sharing feature by passing the ``value_sharing=False`` option to ``dump()``/``dumps()``.

Tag support
-----------

In addition to all standard CBOR tags, this library supports many extended tags:

=== ======================================== ====================================================
Tag Semantics                                Python type(s)
=== ======================================== ====================================================
0   Standard date/time string                datetime.date / datetime.datetime
1   Epoch-based date/time                    datetime.date / datetime.datetime
2   Positive bignum                          int / long
3   Negative bignum                          int / long
4   Decimal fraction                         decimal.Decimal
5   Bigfloat                                 decimal.Decimal
28  Mark shared value                        N/A
29  Reference shared value                   N/A
30  Rational number                          fractions.Fraction
35  Regular expression                       ``_sre.SRE_Pattern`` (result of ``re.compile(...)``)
36  MIME message                             email.message.Message
37  Binary UUID                              uuid.UUID
=== ======================================== ====================================================

Customizing encoding/decoding
-----------------------------

The encoder allows you to specify a mapping of types to callables that handle the encoding of some
otherwise unsupported type. The decoder, on the other hand, allows you to specify a mapping of
semantic tag numbers to callables that implement custom transformation logic for tagged values.

See the docstrings of ``cbor2.CBOREncoder`` and ``cbor2.CBORDecoder`` for details.

Project links
-------------

* `Version history`_
* `Source code`_
* `Issue tracker`_

.. _RFC 7049: https://tools.ietf.org/html/rfc7049
.. _Version history: https://github.com/agronholm/cbor2/blob/master/CHANGES.rst
.. _Source code: https://github.com/agronholm/cbor2
.. _Issue tracker: https://github.com/agronholm/cbor2/issues
