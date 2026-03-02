API reference
=============

Encoding
--------

.. autofunction:: cbor2.dumps
.. autofunction:: cbor2.dump
.. autoclass:: cbor2.CBOREncoder
.. autodecorator:: cbor2.shareable_encoder

Decoding
--------

.. autofunction:: cbor2.loads
.. autofunction:: cbor2.load
.. autoclass:: cbor2.CBORDecoder

Types
-----

.. autoclass:: cbor2.CBORSimpleValue
.. autoclass:: cbor2.CBORTag
.. autoclass:: cbor2.FrozenDict
.. data:: cbor2.break_marker

    A singleton representing the CBOR break marker, used for encoding
    indefinite strings and containers.
.. data:: cbor2.undefined

    A singleton representing the CBOR "undefined" value.

Type aliases
------------

.. type:: MajorDecoderCallback
    :canonical: ~collections.abc.Callable[[CBORDecoder, int], typing.Any]

    Type of the callback needed in the ``major_decoders`` decoder option.
.. type:: SemanticDecoderCallback
    :canonical: ~collections.abc.Callable[[CBORDecoder], Any]

    Type of the callback needed in the ``semantic_decoders`` decoder option.
.. type:: ObjectHook
    :canonical: ~collections.abc.Callable[[CBORDecoder, dict[Any, Any]], Any]

    Type of the callback needed for the ``object_hook`` decoder option.
.. type:: TagHook
    :canonical: ~collections.abc.Callable[[CBORDecoder, CBORTag], Any]

    Type of the callback needed for the ``tag_hook`` decoder option.
.. type:: EncoderHook
    :canonical: ~collections.abc.Callable[[CBOREncoder, Any], Any]

    Type of the callback needed for the ``default`` encoder option.

Exceptions
----------

.. autoexception:: cbor2.CBORError
.. autoexception:: cbor2.CBOREncodeError
.. autoexception:: cbor2.CBOREncodeTypeError
.. autoexception:: cbor2.CBOREncodeValueError
.. autoexception:: cbor2.CBORDecodeError
.. autoexception:: cbor2.CBORDecodeValueError
.. autoexception:: cbor2.CBORDecodeEOF
