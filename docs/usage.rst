Basic usage
===========

.. py:currentmodule:: cbor2

Serializing and deserializing with cbor2 is pretty straightforward::

    from cbor2 import dump, dumps, load, loads

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

Some data types, however, require extra considerations, as detailed below.

Date/time handling
------------------

The CBOR specification does not support naïve datetimes (that is, datetimes where ``tzinfo`` is
missing). When the encoder encounters such a datetime, it needs to know which timezone it belongs
to. To this end, you can specify a default timezone by passing a :class:`~datetime.tzinfo` instance
to :func:`dump`/:func:`dumps` call as the ``timezone`` argument.
Decoded datetimes are always timezone aware.

By default, datetimes are serialized in a manner that retains their timezone offsets. You can
optimize the data stream size by passing ``datetime_as_timestamp=False`` to
:func:`dump`/:func:`dumps`, but this causes the timezone offset
information to be lost.

In versions prior to 4.2 the encoder would convert a ``datetime.date`` object into a
``datetime.datetime`` prior to writing. This can cause confusion on decoding so this has been
disabled by default in the next version. The behaviour can be re-enabled as follows::

    from cbor2 import dumps
    from datetime import date, timezone

    # Serialize dates as datetimes
    encoded = dumps(date(2019, 10, 28), timezone=timezone.utc, date_as_datetime=True)

A default timezone offset must be provided also.

Cyclic (recursive) data structures
----------------------------------

If the encoder encounters a shareable object (ie. list or dict) that it has seen before, it will
by default raise :exc:`CBOREncodeError` indicating that a cyclic reference has been
detected and value sharing was not enabled. CBOR has, however, an extension specification that
allows the encoder to reference a previously encoded value without processing it again. This makes
it possible to serialize such cyclic references, but value sharing has to be enabled by passing
``value_sharing=True`` to :func:`dump`/:func:`dumps`.

.. warning:: Support for value sharing is rare in other CBOR implementations, so think carefully
    whether you want to enable it. It also causes some line overhead, as all potentially shareable
    values must be tagged as such.

String references
-----------------

When ``string_referencing=True`` is passed to
:func:`dump`/:func:`dumps`, if the encoder would encode a string that
it has previously encoded and where a reference would be shorter than the encoded string, it
instead encodes a reference to the nth sufficiently long string already encoded.

.. warning:: Support for string referencing is rare in other CBOR implementations, so think
    carefully whether you want to enable it.

Decoder support for semantic tags
---------------------------------

Below is a list of supported semantic tags for decoding.

===== ======================================== ====================================================
Tag   Semantics                                Python type(s)
===== ======================================== ====================================================
0     Standard datetime string                 :class:`datetime.datetime`
1     Epoch-based datetime                     :class:`datetime.datetime`
2     Positive bignum                          :class:`int`
3     Negative bignum                          :class:`int`
4     Decimal fraction                         :class:`decimal.Decimal`
5     Bigfloat                                 :class:`decimal.Decimal`
25    String reference                         :class:`str` / :class:`bytes`
28    Mark shared value                        N/A
29    Reference shared value                   N/A
30    Rational number                          :class:`fractions.Fraction`
35    Regular expression                       :class:`re.Pattern` (result of ``re.compile(...)``)
36    MIME message                             :class:`email.message.Message`
37    Binary UUID                              :class:`uuid.UUID`
52    IPv4 address/network                     :class:`ipaddress.IPv4Address` or
                                               :class:`ipaddress.IPv4Network`
54    IPv6 address/network                     :class:`ipaddress.IPv6Address` or
                                               :class:`ipaddress.IPv6Network`
100   Epoch-based date                         :class:`datetime.date`
256   String reference namespace               N/A
258   Set of unique items                      :class:`set`
260   Network address (**DEPRECATED**)         :class:`ipaddress.IPv4Address` or
                                               :class:`ipaddress.IPv4Address`
261   Network prefix (**DEPRECATED)**)         :class:`ipaddress.IPv4Network`,
                                               :class:`ipaddress.IPv4Interface`,
                                               :class:`ipaddress.IPv6Network` or
                                               :class:`ipaddress.IPv6Interface`
1004  ISO formatted date string                :class:`datetime.date`
43000 Single complex number                    :class:`complex`
55799 Self-Described CBOR                      object
===== ======================================== ====================================================

Any unsupported semantic tags encountered by the decoder will be decoded as :class:`CBORTag`
objects.

Encoder support for built-in CBOR types
---------------------------------------

Below is the list of Python types encoded as various major CBOR tags or special
values.

================================= ========= ===============================
Python type                       Major tag Subtype
================================= ========= ===============================
:class:`int`                      0 or 1
:class:`bytes`                    2
:class:`bytearray`                2
:class:`str`                      3
:class:`list`                     4
:class:`tuple`                    4
:class:`collections.abc.Sequence` 4
:class:`dict`                     5
:class:`collections.abc.Mapping`  5
:class:`bool`                     7         20 (``False``) or 21 (``True``)
:data:`None`                      7         22
:data:`undefined`                 7         23
:class:`float`                    7         27
:data:`break_marker`              7         31
================================= ========= ===============================

Encoder semantic tag support
----------------------------

Below is the list of Python types and the semantic tags they're encoded as.

================================= =========================================
Python type                       Semantic tag
================================= =========================================
:class:`complex`                  43000
:class:`datetime.datetime`        0 or 1, depending on the
                                  ``datetime_as_timestamp`` setting
:class:`datetime.date`            1004, 100, 1 or 0, depending on the
                                  ``date_as_datetime`` and
                                  ``datetime_as_timestamp`` settings
:class:`decimal.Decimal`          4 (unless NaN or infinities, which are
                                  encoded as floats)
:class:`fractions.Fraction`       30
:class:`frozenset`                258
:class:`re.Pattern`               35
:class:`email.mime.text.MIMEText` 35
:class:`ipaddress.IPv4Address`    52
:class:`ipaddress.IPv4Interface`  52
:class:`ipaddress.IPv4Network`    52
:class:`ipaddress.IPv6Address`    54
:class:`ipaddress.IPv6Interface`  54
:class:`ipaddress.IPv6Network`    54
:class:`set`                      258
:class:`uuid.UUID`                37
================================= =========================================


If you want to write a file that is detected as CBOR by the Unix ``file`` utility, wrap your data
in a :class:`CBORTag` object like so::

    from cbor2 import dump, CBORTag

    with open('output.cbor', 'wb') as fp:
        dump(CBORTag(55799, obj), fp)

This will be ignored on decode and the original data content will be returned.

Use Cases
---------

Here are some things that the cbor2 library could be (and in some cases, is being) used for:

- Experimenting with network protocols based on CBOR encoding
- Designing new data storage formats
- Submitting binary documents to ElasticSearch without base64 encoding overhead
- Storing and validating file metadata in a secure backup system
- RPC which supports Decimals with low overhead
