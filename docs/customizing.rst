Customizing encoding and decoding
=================================

.. py:currentmodule:: cbor2

Both the encoder and decoder can be customized to support a wider range of types.

Customizing the decoder
-----------------------

There are four ways to customize the decoding behavior, available as keyword arguments to
:func:`load`, :func:`loads` and :class:`CBORDecoder`:

#. ``major_decoders``: lets you override how specific major CBOR types are decoded
#. ``semantic_decoders``: lets you override how specific semantic tags are decoded
#. ``tag_hook``: lets you define a catch-all for unhandled semantic tags
#. ``object_hook``: lets you transform any newly-decoded dicts

Overriding the decoding of major types
++++++++++++++++++++++++++++++++++++++

The following example overrides the decoding for major type 3 (text strings)::

    import cbor2

    def string_decoder(decoder: cbor2.CBORDecoder, subtype: int) -> str:
        # Call the original implementation
        # (optional if you want to do the low-level decoding yourself)
        my_string = decoder.decode_string(subtype)
        return my_string[::-1]

    payload = cbor2.dumps("Hello, world!")
    assert cbor2.loads(payload, major_decoders={3: string_decoder}) == "!dlrow ,olleH"

.. note:: Overriding major decoders is a niche feature, not needed by most users.
    Additionally, passing a major decoder lookup mapping has negative consequences
    for the decoder's performance due to the extra round-trips to the Python interpreter.

Overriding the decoding of semantic tags
++++++++++++++++++++++++++++++++++++++++

If you want to override how an already supported tag is decoded, this is a good way to do it.

Here's an example decoder implementation for semantic tag 1 (epoch datetime)::

    from datetime import datetime, timezone

    import cbor2

    def decode_epoch_datetime(decoder: cbor2.CBORDecoder) -> datetime:
        timestamp = decoder.decode()
        return datetime.fromtimestamp(timestamp, timezone.utc)

    payload = cbor2.dumps(cbor2.CBORTag(1, 1363896240))
    decoded = cbor2.loads(payload, semantic_decoders={1: decode_epoch_datetime})
    assert decoded == datetime(2013, 3, 21, 20, 4, tzinfo=timezone.utc)

.. note:: Overriding semantic decoders incurs a slight performance penalty for all semantic
    tags as it involves a round-trip to the Python interpreter for the decoder callback
    lookup.

Specifying a "catch-all" for unhandled semantic tags
++++++++++++++++++++++++++++++++++++++++++++++++++++

By specifying a ``tag_hook``, the decoder will handle otherwise unhandled semantic tags by calling
this callable with two arguments: the decoder instance and the tag object. Its return value is used
in place of the :class:`CBORTag` object that would have otherwise been returned.

Here's an example that assumes semantic tag 4000 to contain an array of attributes ``x`` and ``y``
for a custom ``Point`` class::

    import cbor2

    class Point:
        def __init__(self, x: int, y: int):
            self.x = x
            self.y = y

    def tag_hook(decoder: cbor2.CBORDecoder, tag: cbor2.CBORTag) -> Point | cbor2.CBORTag:
        if tag.tag == 4000:
            # we expect tag.value to be an array of [x, y] attributes
            return Point(*tag.value)

        return tag

    payload = cbor2.dumps(cbor2.CBORTag(4000, [4, 5]))
    point = cbor2.loads(payload, tag_hook=tag_hook)
    assert isinstance(point, Point)
    assert point.x == 4
    assert point.y == 5

Customizing map decoding
++++++++++++++++++++++++

The final decoder option allows users to customize how CBOR maps are decoded, using the
``object_hook`` option. This callback takes two arguments: the decoder instance and a
:class:`dict`. The callback should return either the dictionary passed to it, or another object
that should replace it.

Here's an example that decode any dict with the key ``typename`` set to ``Point`` as a ``Point``
instance::

    from typing import Any

    import cbor2

    class Point:
        def __init__(self, x: int, y: int):
            self.x = x
            self.y = y

    def object_hook(decoder: cbor2.CBORDecoder, value: dict[Any, Any]) -> dict[Any, Any] | Point:
        if value.get("typename") == "Point":
            return Point(value["x"], value["y"])

        return value

    payload = cbor2.dumps({"typename": "Point", "x": 4, "y": 5})
    point = cbor2.loads(payload, object_hook=object_hook)
    assert isinstance(point, Point)
    assert point.x == 4
    assert point.y == 5

.. note:: Make sure you have well defined rules for special handling of dicts so you don't end up
    trying to convert all CBOR maps the decoder encounters.

Dealing with immutable containers
+++++++++++++++++++++++++++++++++

In rare cases, you may need to decode the next item from the stream as immutable.
In practice, this means:

* Arrays are decoded as :class:`tuple` instead of :class:`list`
* Maps are decoded as :class:`~cbor2.FrozenDict` instead of :class:`dict`
* Sets are decoded as :class:`set` instead of :class:`frozenset`

TODO: write the rest of the section

Customizing the encoder
-----------------------

There are two ways to customize the encoder behavior available as keyword arguments to
:func:`dump`, :func:`dumps` and :class:`CBOREncoder`:

* ``encoders``: specifies a mapping of an **exact** Python type to an encoder callable
* ``default``: specifies a "catch-all" encoder callable for objects not matched with any specific
  encoder callback

Overriding the encoder for a specific Python type
+++++++++++++++++++++++++++++++++++++++++++++++++

The ``encoders`` option allows users to override the encoding behavior for any Python types.
The option takes a :class:`dict` or any :class:`mapping type <collections.abc.Mapping>` where the
keys are Python types and the values are encoder callbacks. The encoder callbacks must take two
positional arguments: the encoder instance and the object to be encoded.

Here's an example of how to add support for encoding a custom type::

    import cbor2

    class Point:
        def __init__(self, x: int, y: int):
            self.x = x
            self.y = y

    def encode_point(encoder: cbor2.CBOREncoder, value: Point) -> None:
        # Tag number 4000 was chosen arbitrarily
        encoder.encode_semantic(4000, [value.x, value.y])

    # prints b'\xd9\x0f\xa0\x82\x04\x05'
    print(cbor2.dumps(Point(4, 5), encoders={Point: encode_point}))

This encodes the two fields, x and y, as an array under the (arbitrarily chosen) semantic tag 4000.

.. important:: The encoder matches type **exactly**, so it will not match against subclasses of
    types in the encoder registry!

Specifying a "catch-all" callback for unhandled semantic tags
+++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++

The ``default`` option is used by the encoder as the last resort for any objects it could not
encode otherwise. Just like with the ``encoders`` option, the callback takes two arguments: the
encoder instance and the object to be encoded.

Here's an example that tries to encode arbitrary objects as a combination of the fully qualified
class name and a dictionary of attribute values::

    import cbor2

    class Point:
        def __init__(self, x: int, y: int):
            self.x = x
            self.y = y

    def default_encoder(encoder: cbor2.CBOREncoder, obj: object) -> None:
        cls = obj.__class__
        name = f"{cls.__module__}.{cls.__qualname__}"
        attributes = {key: getattr(obj, key) for key in dir(obj)}
        encoder.encode_semantic(50000, [name, attributes])

    # prints b'\xd9\xc3P\x82n__main__.Point\xa2ax\x04ay\x05'
    print(cbor2.dumps(Point(4, 5), default=default_encoder))

Value sharing with custom types
-------------------------------

In order to properly encode and decode cyclic references with custom types, some special care has
to be taken. Suppose you have a custom type as below, where any child object could contain a
reference to its parent or any ancestor, you would encounter an error when naively trying to
serialize such a cyclic object graph::

    from __future__ import annotations

    import cbor2

    class MyType:
        def __init__(self, parent: MyType | None = None):
            self.parent = parent
            self.children = []
            if parent:
                self.parent.children.append(self)

    def encode_mytype(encoder: cbor2.CBOREncoder, value: MyType):
        # The state has to be serialized separately so that the decoder would have a chance to
        # create an empty instance before the shared value references are decoded
        encoder.encode_semantic(3000, value.__dict__)

    def decode_mytype(decoder: cbor2.CBORDecoder) -> MyType:
        instance = MyType.__new__()
        state = decoder.decode()
        instance.__dict__.update(state)
        return instance

    parent = MyType()
    child1 = MyType(parent)
    child2 = MyType(parent)
    # ERROR: cbor2.CBOREncodeValueError: cyclic data structure detected
    serialized = cbor2.dumps(parent, encoders={MyType: encode_mytype})

To fix this, a few adjustments need to be made:

#. Value sharing needs to be turned on in the encoder with ``value_sharing=True``
#. The encoder callback must be decorated with :deco:`shareable_encoder`
#. The decoder callback must call :meth:`CBORDecoder.set_shareable` with ``instance``
   (the "empty" instance before its state has been decoded) as the argument

Here is the revised example::

    from __future__ import annotations

    import cbor2

    class MyType:
        def __init__(self, parent: MyType | None = None):
            self.parent = parent
            self.children = []
            if parent:
                self.parent.children.append(self)

    @cbor2.shareable_encoder
    def encode_mytype(encoder: cbor2.CBOREncoder, value: MyType):
        # The state has to be serialized separately so that the decoder would have a chance to
        # create an empty instance before the shared value references are decoded
        encoder.encode_semantic(3000, value.__dict__)

    def decode_mytype(decoder: cbor2.CBORDecoder) -> MyType:
        # Connects the new MyType instance with the first available shareable index
        # so that later references to that index can get the same object
        instance = decoder.set_shareable(MyType.__new__(MyType))

        # The decoded state can now contain references to the empty instance
        state = decoder.decode()
        instance.__dict__.update(state)
        return instance

    parent = MyType()
    child1 = MyType(parent)
    child2 = MyType(parent)
    # Important: value sharing must be enabled
    serialized = cbor2.dumps(parent, encoders={MyType: encode_mytype}, value_sharing=True)

    new_parent = cbor2.loads(serialized, semantic_decoders={3000: decode_mytype})
    assert new_parent.children[0].parent is new_parent
    assert new_parent.children[1].parent is new_parent
