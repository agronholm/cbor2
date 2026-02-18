Customizing encoding and decoding
=================================

.. py:currentmodule:: cbor2

Both the encoder and decoder can be customized to support a wider range of types.

On the encoder side, this is accomplished by passing a callback as the ``default`` constructor
argument. This callback will receive an object that the encoder could not serialize on its own.
The callback should then return a value that the encoder can serialize on its own, although the
return value is allowed to contain objects that also require the encoder to use the callback, as
long as it won't result in an infinite loop.

On the decoder side, you have four venues, available as keyword arguments to :func:`load`,
:func:`loads` and :class:`CBORDecoder`:

#. ``major_decoders``: lets you override how specific major CBOR types are decoded
#. ``semantic_decoders``: lets you override how specific semantic tags are decoded
#. ``tag_hook``: lets you define a catch-all for unhandled semantic tags
#. ``object_hook``: lets you transform any newly-decoded dicts

Overriding the decoding of major types
--------------------------------------

The following example overrides the decoding for major type 3 (text strings)::

    import cbor2

    def string_decoder(decoder, subtype):
        # Call the original implementation (optional if you want to do the
        # low-level decoding yourself
        my_string = decoder.decode_string(subtype)
        return my_string[::-1]

    payload = b'mHello, world!'  # "Hello, world" encoded with CBOR
    print(cbor2.loads(payload, major_decoders={3: string_decoder}))
    # This prints: !dlrow ,olleH

.. note:: Overriding major decoders is a niche feature, not needed by most users.
    Additionally, passing a major decoder lookup mapping has negative consequences
    for the decoder's performance due to the extra round-trips to the Python interpreter.

Overriding the decoding of semantic tags
----------------------------------------

If you want to override how an already supported tag is decoded, this is a good way to do it.

Here's an example decoder implementation for semantic tag 1 (epoch datetime)::

    from datetime import datetime, timezone

    import cbor2

    def decode_epoch_datetime(decoder):
        timestamp = decoder.decode()
        return datetime.fromtimestamp(timestamp, timezone.utc)

    payload = b'\xc1\x1aQKg\xb0'  # 1(1363896240) in CBOR notation
    print(cbor2.loads(payload, semantic_decoders={1: decode_epoch_datetime}))
    # This prints: 2013-03-21 20:04:00+00:00

.. note:: Overriding semantic decoders incurs a slight performance penalty for all semantic
    tags as it involves a round-trip to the Python interpreter for the decoder callback
    lookup.

Using the CBOR tags for custom types
------------------------------------

The most common way to use ``default`` is to call :meth:`CBOREncoder.encode`
to add a custom tag in the data stream, with the payload as the value::

    class Point:
        def __init__(self, x, y):
            self.x = x
            self.y = y

    def default_encoder(encoder, value):
        # Tag number 4000 was chosen arbitrarily
        encoder.encode(CBORTag(4000, [value.x, value.y]))

The corresponding ``tag_hook`` would be::

    def tag_hook(decoder, tag):
        if tag.tag != 4000:
            return tag

        # tag.value is now the [x, y] list we serialized before
        return Point(*tag.value)

Using dicts to carry custom types
---------------------------------

The same could be done with ``object_hook``, except less efficiently::

    def default_encoder(encoder, value):
        encoder.encode(dict(typename='Point', x=value.x, y=value.y))

    def object_hook(decoder, value):
        if value.get('typename') != 'Point':
            return value

        return Point(value['x'], value['y'])

You should make sure that whatever way you decide to use for telling apart your "specially marked"
dicts from arbitrary data dicts won't mistake on for the other.

Value sharing with custom types
-------------------------------

In order to properly encode and decode cyclic references with custom types, some special care has
to be taken. Suppose you have a custom type as below, where every child object contains a reference
to its parent and the parent contains a list of children::

    from cbor2 import dumps, loads, shareable_encoder, CBORTag


    class MyType:
        def __init__(self, parent=None):
            self.parent = parent
            self.children = []
            if parent:
                self.parent.children.append(self)

This would not normally be serializable, as it would lead to an endless loop (in the worst case)
and raise some exception (in the best case). Now, enter CBOR's extension tags 28 and 29. These tags
make it possible to add special markers into the data stream which can be later referenced and
substituted with the object marked earlier.

To do this, in ``default`` hooks used with the encoder you will need to use the
:meth:`shareable_encoder` decorator on your ``default`` hook function. It will
automatically automatically add the object to the shared values registry on the encoder and prevent
it from being serialized twice (instead writing a reference to the data stream)::

    @shareable_encoder
    def default_encoder(encoder, value):
        # The state has to be serialized separately so that the decoder would have a chance to
        # create an empty instance before the shared value references are decoded
        serialized_state = encoder.encode_to_bytes(value.__dict__)
        encoder.encode(CBORTag(3000, serialized_state))

On the decoder side, you will need to initialize an empty instance for shared value lookup before
the object's state (which may contain references to it) is decoded.
This is done with the :meth:`CBORDecoder.set_shareable` method::

    def tag_hook(decoder, tag, shareable_index=None):
        # Return all other tags as-is
        if tag.tag != 3000:
            return tag

        # Create a raw instance before initializing its state to make it possible for cyclic
        # references to work
        instance = MyType.__new__(MyType)
        decoder.set_shareable(shareable_index, instance)

        # Separately decode the state of the new object and then apply it
        state = decoder.decode_from_bytes(tag.value)
        instance.__dict__.update(state)
        return instance

You could then verify that the cyclic references have been restored after deserialization::

    parent = MyType()
    child1 = MyType(parent)
    child2 = MyType(parent)
    serialized = dumps(parent, default=default_encoder, value_sharing=True)

    new_parent = loads(serialized, tag_hook=tag_hook)
    assert new_parent.children[0].parent is new_parent
    assert new_parent.children[1].parent is new_parent

Decoding Tagged items as keys
-----------------------------

Since the CBOR specification allows any type to be used as a key in the mapping type, the decoder
provides a flag that indicates it is expecting an immutable (and by implication hashable) type. If
your custom class cannot be used this way you can raise an exception if this flag is set::

    def tag_hook(decoder, tag):
        if tag.tag != 3000:
            return tag

        if decoder.immutable:
            raise CBORDecodeException('MyType cannot be used as a key or set member')

        return MyType(*tag.value)

An example where the data could be used as a dict key::

    from collections import namedtuple

    Pair = namedtuple('Pair', 'first second')

    def tag_hook(decoder, tag):
        if tag.tag != 4000:
            return tag

        return Pair(*tag.value)

The ``object_hook`` can check for the immutable flag in the same way.
