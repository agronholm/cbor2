import math
import re
import struct
from calendar import timegm
from collections import OrderedDict, Sequence, Mapping
from datetime import datetime, time, date
from decimal import Decimal
from email.message import Message
from fractions import Fraction
from functools import wraps
from io import BytesIO
from uuid import UUID

from cbor2.compat import iteritems, timezone, long, unicode, as_unicode
from cbor2.types import CBORTag, undefined


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


def shareable_encoder(func):
    """
    Wrap the given encoder function to gracefully handle cyclic data structures.

    If value sharing is enabled, this marks the given value shared in the datastream on the
    first call. If the value has already been passed to this method, a reference marker is
    instead written to the data stream and the wrapped function is not called.

    If value sharing is disabled, only infinite recursion protection is done.

    """
    @wraps(func)
    def wrapper(encoder, value, fp, *args, **kwargs):
        value_id = id(value)
        if encoder.value_sharing:
            container_index = encoder.container_indexes.get(value_id)
            if container_index is None:
                # Mark the container as shareable
                encoder.container_indexes[value_id] = len(encoder.container_indexes)
                fp.write(encode_length(0xd8, 0x1c))
                func(encoder, value, fp, *args, **kwargs)
            else:
                # Generate a reference to the previous index instead of encoding this again
                fp.write(encode_length(0xd8, 0x1d))
                encoder.encode_int(container_index, fp)
        else:
            if value_id in encoder.container_indexes:
                raise CBOREncodeError('cyclic data structure detected but value sharing is '
                                      'disabled')
            else:
                encoder.container_indexes[value_id] = None
                func(encoder, value, fp, *args, **kwargs)
                del encoder.container_indexes[value_id]

    return wrapper


class CBOREncoder(object):
    """
    Serializes objects to bytestrings using Concise Binary Object Representation.

    The following parameters are also available as attributes on the encoder:

    :param datetime_as_timestamp: set to ``True`` to serialize datetimes as UNIX timestamps
        (this makes datetimes more concise on the wire but loses the time zone information)
    :param datetime.tzinfo timezone: the default timezone to use for serializing naive
        datetimes
    :param value_sharing: set to ``False`` to disable value sharing (this will cause an error
        when a cyclic data structure is encountered)
    :param encoders: a mapping of type -> encoder callable. The encoder callable receives three
        arguments: the CBOREncoder instance, the value to be encoded and the output file object.
        The callable must either write directly to the file object or call another encoding method
        that does the output.

        To support cyclic references, you can wrap the encoder callback with
        ``@shareable_encoder``. A useful strategy is to encode the state of the object separately
        and returning the resulting bytestring to the encoder.
    """

    def __init__(self, datetime_as_timestamp=False, timezone=None, value_sharing=True,
                 encoders=None):
        self.datetime_as_timestamp = datetime_as_timestamp
        self.timezone = timezone
        self.value_sharing = value_sharing
        self.container_indexes = {}

        # Apply custom encoders
        if encoders:
            self.encoders = self.default_encoders.copy()
            self.encoders.update(encoders)
        else:
            self.encoders = self.default_encoders

    def encode_int(self, value, fp):
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
            self.encode_semantic(major_type, bytestring, fp)
        elif value >= 0:
            fp.write(encode_length(0, value))
        else:
            fp.write(encode_length(0x20, abs(value) - 1))

    def encode_bytestring(self, value, fp):
        fp.write(encode_length(0x40, len(value)))
        fp.write(value)

    def encode_bytearray(self, value, fp):
        self.encode_bytestring(bytes(value), fp)

    def encode_string(self, value, fp):
        value = value.encode('utf-8')
        fp.write(encode_length(0x60, len(value)))
        fp.write(value)

    @shareable_encoder
    def encode_array(self, value, fp):
        fp.write(encode_length(0x80, len(value)))
        for item in value:
            self.encode(item, fp)

    @shareable_encoder
    def encode_map(self, value, fp):
        fp.write(encode_length(0xa0, len(value)))
        for key, value in iteritems(value):
            self.encode(key, fp)
            self.encode(value, fp)

    def encode_semantic(self, tag, value, fp, disable_value_sharing=False):
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

        fp.write(encode_length(0xc0, tag))
        self.encode(value, fp)

        if disable_value_sharing:
            self.value_sharing = value_sharing

    #
    # Semantic decoders (major tag 6)
    #

    def encode_datetime(self, value, fp):
        # Semantic tag 0
        if not value.tzinfo:
            if self.timezone:
                value = value.replace(tzinfo=self.timezone)
            else:
                raise CBOREncodeError(
                    'naive datetime encountered and no default timezone has been set')

        if self.datetime_as_timestamp:
            timestamp = timegm(value.utctimetuple()) + value.microsecond // 1000000
            self.encode_semantic(1, timestamp, fp)
        else:
            datestring = as_unicode(value.isoformat().replace('+00:00', 'Z'))
            self.encode_semantic(0, datestring, fp)

    def encode_date(self, value, fp):
        value = datetime.combine(value, time()).replace(tzinfo=timezone.utc)
        self.encode_datetime(value, fp)

    def encode_decimal(self, value, fp):
        # Semantic tag 4
        if value.is_nan():
            fp.write(b'\xf9\x7e\x00')
        elif value.is_infinite():
            fp.write(b'\xf9\x7c\x00' if value > 0 else b'\xf9\xfc\x00')
        else:
            dt = value.as_tuple()
            mantissa = sum(d * 10 ** i for i, d in enumerate(reversed(dt.digits)))
            self.encode_semantic(4, [dt.exponent, mantissa], fp, True)

    def encode_rational(self, value, fp):
        # Semantic tag 30
        self.encode_semantic(30, [value.numerator, value.denominator], fp, True)

    def encode_regexp(self, value, fp):
        # Semantic tag 35
        self.encode_semantic(35, as_unicode(value.pattern), fp)

    def encode_mime(self, value, fp):
        # Semantic tag 36
        self.encode_semantic(36, as_unicode(value.as_string()), fp)

    def encode_uuid(self, value, fp):
        # Semantic tag 37
        self.encode_semantic(37, value.bytes, fp)

    def encode_custom_tag(self, value, fp):
        # CBORTag (for arbitrary unsupported tags)
        self.encode_semantic(value.tag, value.value, fp)

    #
    # Special encoders (major tag 7)
    #

    def encode_float(self, value, fp):
        # Handle special values efficiently
        if math.isnan(value):
            fp.write(b'\xf9\x7e\x00')
        elif math.isinf(value):
            fp.write(b'\xf9\x7c\x00' if value > 0 else b'\xf9\xfc\x00')
        else:
            fp.write(struct.pack('>Bd', 0xfb, value))

    def encode_boolean(self, value, fp):
        fp.write(b'\xf5' if value else b'\xf4')

    def encode_none(self, value, fp):
        fp.write(b'\xf6')

    def encode_undefined(self, value, fp):
        fp.write(b'\xf7')

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
        (type(undefined), encode_undefined),
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

    def encode(self, obj, fp):
        """
        Encode the given object using CBOR.

        This method looks up the proper encoding callback and then calls it with this encoder
        instance and the object as arguments. The callback is expected to either write the results
        directly to the encoder's output stream (``fp``) or call another function that does so.

        """
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

        encoder(self, obj, fp)


def dumps(obj, **kwargs):
    """
    Serialize an object to a bytestring.

    :param obj: the object to serialize
    :param kwargs: keyword arguments passed to ``CBOREncoder()``
    :return: the serialized output
    :rtype: bytes

    """
    buf = BytesIO()
    CBOREncoder(**kwargs).encode(obj, buf)
    return buf.getvalue()


def dump(obj, fp, **kwargs):
    """
    Serialize an object to a file.

    :param obj: the object to serialize
    :param BinaryIO fp: a file-like object
    :param kwargs: keyword arguments passed to ``CBOREncoder()``

    """
    CBOREncoder(**kwargs).encode(obj, fp)
