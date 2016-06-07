import math
import re
import struct
from calendar import timegm
from collections import OrderedDict, Sequence, Mapping
from contextlib import contextmanager
from datetime import datetime, time, date
from decimal import Decimal
from email.message import Message
from fractions import Fraction
from uuid import UUID

from cbor2.compat import iteritems, timezone, long, unicode, as_unicode
from cbor2.types import CBORTag


class CBOREncodeError(Exception):
    """Raised when an error occurs while serializing an object into a CBOR datastream."""


def encode_length(major_tag, length):
    if length < 24:
        return struct.pack('>B', major_tag | length)
    elif length < 256:
        return struct.pack('>BB', major_tag | 24, length)
    elif length < 65536:
        return struct.pack('>BH', major_tag | 25, length)
    elif length < 4294967296:
        return struct.pack('>BL', major_tag | 26, length)
    else:
        return struct.pack('>BQ', major_tag | 27, length)


class CBOREncoder(object):
    """
    Serializes objects to bytestrings using Concise Binary Object Representation.

    :param datetime_as_timestamp: set to ``True`` to serialize datetimes as UNIX timestamps
        (this makes datetimes more concise on the wire but loses the time zone information)
    :param datetime.tzinfo timezone: the default timezone to use for serializing naive
        datetimes
    :param value_sharing: set to ``False`` to disable value sharing (this will cause an error
        when a cyclic data structure is encountered)
    :param encoders: a mapping of type -> encoder callable. The encoder callable receives two
        arguments: CBOREncoder instance and value. The callable must return an iterable of
        bytestrings.

    :ivar Set[int] container_stack: set of container ids (``id(...)``) that are present in the
        current container tree
    """

    def __init__(self, datetime_as_timestamp=False, timezone=None, value_sharing=True,
                 encoders=None):
        self.datetime_as_timestamp = datetime_as_timestamp
        self.timezone = timezone
        self.value_sharing = value_sharing
        self.container_indexes = {}
        self.container_stack = set()

        # Apply custom encoders
        if encoders:
            self.encoders = self.default_encoders.copy()
            self.encoders.update(encoders)
        else:
            self.encoders = self.default_encoders

    @contextmanager
    def _in_stack(self, container_id):
        self.container_stack.add(container_id)
        yield
        self.container_stack.remove(container_id)

    def encode_int(self, value):
        # Big integers (2 ** 64 and over)
        if value >= 18446744073709551616 or value < -18446744073709551616:
            if value >= 0:
                major_type = 0x02
            else:
                major_type = 0x03
                value = -value - 1

            values = []
            while value > 0:
                value, remainder = divmod(value, 256)
                values.insert(0, remainder)

            bytestring = struct.pack('>%dB' % len(values), *values)
            for chunk in self.encode_semantic(major_type, bytestring):
                yield chunk

            return

        if value >= 0:
            major_tag = 0
        else:
            major_tag = 0x20
            value = -value - 1

        yield encode_length(major_tag, value)

    def encode_bytestring(self, value):
        return encode_length(0x40, len(value)), value

    def encode_bytearray(self, value):
        return self.encode_bytestring(bytes(value))

    def encode_string(self, value):
        value = value.encode('utf-8')
        return encode_length(0x60, len(value)), value

    def encode_array(self, value):
        value_id = id(value)
        if self.value_sharing:
            container_index = self.container_indexes.get(value_id)
            if container_index is None:
                # Mark the container as shareable
                self.container_indexes[value_id] = len(self.container_stack)
                yield encode_length(0xd8, 0x1c)
            else:
                # Generate a reference to the previous index instead of encoding this again
                yield encode_length(0xd8, 0x1d)
                for chunk in self.encode_int(container_index):
                    yield chunk

                return
        elif value_id in self.container_stack:
            raise CBOREncodeError('cyclic data structure detected but value sharing is '
                                  'disabled')

        yield encode_length(0x80, len(value))
        with self._in_stack(value_id):
            for item in value:
                for chunk in self.encode(item):
                    yield chunk

    def encode_map(self, value):
        value_id = id(value)
        if self.value_sharing:
            container_index = self.container_indexes.get(value_id)
            if container_index is None:
                # Mark the container as shareable
                self.container_indexes[value_id] = len(self.container_stack)
                yield encode_length(0xd8, 0x1c)
            else:
                # Generate a reference to the previous index instead of encoding this again
                yield encode_length(0xd8, 0x1d)
                for chunk in self.encode_int(container_index):
                    yield chunk

                return
        elif value_id in self.container_stack:
            raise CBOREncodeError('cyclic data structure detected but value sharing is '
                                  'disabled')

        yield encode_length(0xa0, len(value))
        with self._in_stack(value_id):
            for key, value in iteritems(value):
                for chunk in self.encode(key):
                    yield chunk
                for chunk in self.encode(value):
                    yield chunk

    def encode_semantic(self, tag, value, disable_value_sharing=False):
        """
        Encode the given object as a tagged value.

        :param int tag: the semantic tag to use
        :param value: the value to associate with the tag
        :param bool disable_value_sharing: when ``True``, temporarily disable value sharing. Use
            when you know there will be no recursion involved in ``value``.

        """
        value_sharing = self.value_sharing
        if disable_value_sharing:
            self.value_sharing = False

        yield encode_length(0xc0, tag)
        for chunk in self.encode(value):
            yield chunk

        if disable_value_sharing:
            self.value_sharing = value_sharing

    #
    # Semantic decoders (major tag 6)
    #

    def encode_datetime(self, value):
        # Semantic tag 0
        if not value.tzinfo:
            if self.timezone:
                value = value.replace(tzinfo=self.timezone)
            else:
                raise CBOREncodeError(
                    'naive datetime encountered and no default timezone has been set')

        if self.datetime_as_timestamp:
            timestamp = timegm(value.utctimetuple()) + value.microsecond // 1000000
            return self.encode_semantic(1, timestamp)
        else:
            datestring = as_unicode(value.isoformat().replace('+00:00', 'Z'))
            return self.encode_semantic(0, datestring)

    def encode_date(self, value):
        value = datetime.combine(value, time()).replace(tzinfo=timezone.utc)
        return self.encode_datetime(value)

    def encode_decimal(self, value):
        # Semantic tag 4
        if value.is_nan():
            return b'\xf9\x7e\x00',
        elif value.is_infinite():
            return (b'\xf9\x7c\x00',) if value > 0 else (b'\xf9\xfc\x00',)

        dt = value.as_tuple()
        mantissa = sum(d * 10 ** i for i, d in enumerate(reversed(dt.digits)))
        return self.encode_semantic(4, [dt.exponent, mantissa], True)

    def encode_rational(self, value):
        # Semantic tag 30
        return self.encode_semantic(30, [value.numerator, value.denominator], True)

    def encode_regexp(self, value):
        # Semantic tag 35
        return self.encode_semantic(35, as_unicode(value.pattern))

    def encode_mime(self, value):
        # Semantic tag 36
        return self.encode_semantic(36, as_unicode(value.as_string()))

    def encode_uuid(self, value):
        # Semantic tag 37
        return self.encode_semantic(37, value.bytes)

    def encode_custom_tag(self, value):
        # CBORTag (for arbitrary unsupported tags)
        return self.encode_semantic(value.tag, value.value)

    #
    # Special encoders (major tag 7)
    #

    def encode_float(self, value):
        # Handle special values efficiently
        if math.isnan(value):
            return b'\xf9\x7e\x00',
        elif math.isinf(value):
            return (b'\xf9\x7c\x00',) if value > 0 else (b'\xf9\xfc\x00',)

        return b'\xfb' + struct.pack('>d', value),

    def encode_boolean(self, value):
        return (b'\xf5',) if value else (b'\xf4',)

    def encode_none(self, value):
        return b'\xf6',

    default_encoders = OrderedDict([
        (unicode, encode_string),
        (bytes, encode_bytestring),
        (bytearray, encode_bytearray),
        (int, encode_int),
        (long, encode_int),
        (float, encode_float),
        (Decimal, encode_decimal),
        (bool, encode_boolean),
        (type(None), encode_none),
        (tuple, encode_array),
        (list, encode_array),
        (dict, encode_map),
        (Mapping, encode_map),
        (Sequence, encode_array),
        (datetime, encode_datetime),
        (date, encode_date),
        (type(re.compile('')), encode_regexp),
        (Fraction, encode_rational),
        (Message, encode_mime),
        (UUID, encode_uuid),
        (CBORTag, encode_custom_tag)
    ])

    def encode(self, obj):
        obj_type = obj.__class__
        encoder = self.encoders.get(obj_type)
        if encoder is None:
            # No direct hit -- do a slower subclass check
            for type_, enc in iteritems(self.encoders):
                if issubclass(obj_type, type_):
                    encoder = enc
                    break
            else:
                raise CBOREncodeError('cannot serialize type %s' % obj_type.__name__)

        for chunk in encoder(self, obj):
            yield chunk


def dumps(obj, **kwargs):
    """
    Serialize an object to a bytestring.

    :param obj: the object to serialize
    :param kwargs: keyword arguments passed to ``CBOREncoder()``
    :return: the serialized output
    :rtype: bytes

    """
    encoder = CBOREncoder(**kwargs)
    return b''.join(encoder.encode(obj))


def dump(obj, fp, **kwargs):
    """
    Serialize an object to a file.

    :param obj: the object to serialize
    :param BinaryIO fp: a file-like object
    :param kwargs: keyword arguments passed to ``CBOREncoder()``

    """
    encoder = CBOREncoder(**kwargs)
    for chunk in encoder.encode(obj):
        fp.write(chunk)
