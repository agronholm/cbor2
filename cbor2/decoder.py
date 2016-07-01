import re
import struct
from datetime import datetime, timedelta
from decimal import Decimal
from email.parser import Parser
from fractions import Fraction
from io import BytesIO
from math import ldexp
from uuid import UUID

from cbor2.compat import timezone, xrange, PY2, byte_as_integer
from cbor2.types import CBORTag, undefined, break_marker

timestamp_re = re.compile(r'^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)'
                          r'(?:\.(\d+))?(?:Z|([+-]\d\d):(\d\d))$')


class CBORDecodeError(Exception):
    """Raised when an error occurs deserializing a CBOR datastream."""


class CBORDecoder(object):
    """
    Deserializes objects from a bytestring.

    :param Dict[int, Callable] semantic_decoders: a mapping of semantic tag -> decoder callable.
        The callable receives four arguments: the CBORDecoder instance, the tagged value, the
        output file object and the shareable index for the decoded value if it is marked shareable.
        The callable's return value should be the transformed value.

        If the shareable index value is not ``None``, the decoder callback needs to set the raw
        value in the decoder's shareables *before* doing any further decoding. In other words, if
        the callback constructs an object of some class, it must set
        ``decoder.shareables[shareable_index]`` to the object instance before populating the
        instance. This is necessary to properly support cyclic data structures.
    """

    def __init__(self, semantic_decoders=None):
        self.shareables = []

        if semantic_decoders:
            self.semantic_decoders = self.default_semantic_decoders.copy()
            self.semantic_decoders.update(semantic_decoders)
        else:
            self.semantic_decoders = self.default_semantic_decoders

    def decode_uint(self, subtype, fp, shareable_index=None, allow_infinite=False):
        # Major tag 0
        if subtype < 24:
            return subtype
        elif subtype == 24:
            return struct.unpack('>B', fp.read(1))[0]
        elif subtype == 25:
            return struct.unpack('>H', fp.read(2))[0]
        elif subtype == 26:
            return struct.unpack('>L', fp.read(4))[0]
        elif subtype == 27:
            return struct.unpack('>Q', fp.read(8))[0]
        elif subtype == 31 and allow_infinite:
            return None
        else:
            raise CBORDecodeError('unknown unsigned integer subtype 0x%x' % subtype)

    def decode_negint(self, subtype, fp, shareable_index=None):
        # Major tag 1
        uint = self.decode_uint(subtype, fp)
        return -uint - 1

    def decode_bytestring(self, subtype, fp, shareable_index=None):
        # Major tag 2
        length = self.decode_uint(subtype, fp, allow_infinite=True)
        if length is None:
            # Indefinite length
            buf = bytearray()
            while True:
                initial_byte = byte_as_integer(fp.read(1))
                if initial_byte == 255:
                    return buf
                else:
                    length = self.decode_uint(initial_byte & 31, fp)
                    value = fp.read(length)
                    buf.extend(value)
        else:
            return fp.read(length)

    def decode_string(self, subtype, fp, shareable_index=None):
        # Major tag 3
        return self.decode_bytestring(subtype, fp).decode('utf-8')

    def decode_array(self, subtype, fp, shareable_index=None):
        # Major tag 4
        items = []
        if shareable_index is not None:
            self.shareables[shareable_index] = items

        length = self.decode_uint(subtype, fp, allow_infinite=True)
        if length is None:
            # Indefinite length
            while True:
                value = self.decode(fp)
                if value is break_marker:
                    break
                else:
                    items.append(value)
        else:
            for _ in xrange(length):
                item = self.decode(fp)
                items.append(item)

        return items

    def decode_map(self, subtype, fp, shareable_index=None):
        # Major tag 5
        dictionary = {}
        if shareable_index is not None:
            self.shareables[shareable_index] = dictionary

        length = self.decode_uint(subtype, fp, allow_infinite=True)
        if length is None:
            # Indefinite length
            while True:
                key = self.decode(fp)
                if key is break_marker:
                    break
                else:
                    value = self.decode(fp)
                    dictionary[key] = value
        else:
            for _ in xrange(length):
                key = self.decode(fp)
                value = self.decode(fp)
                dictionary[key] = value

        return dictionary

    def decode_semantic(self, subtype, fp, shareable_index=None):
        # Major tag 6
        tag = self.decode_uint(subtype, fp)

        # Special handling for the "shareable" tag
        if tag == 28:
            shareable_index = len(self.shareables)
            self.shareables.append(None)
            return self.decode(fp, shareable_index)

        value = self.decode(fp)
        try:
            decoder = self.semantic_decoders[tag]
        except KeyError:
            # No special handling available
            return CBORTag(tag, value)

        return decoder(self, value, fp, shareable_index)

    def decode_special(self, subtype, fp, shareable_index=None):
        # Major tag 7
        return self.special_decoders[subtype](self, fp, shareable_index=None)

    #
    # Semantic decoders (major tag 6)
    #

    def decode_datetime_string(self, value, fp, shareable_index=None):
        # Semantic tag 0
        match = timestamp_re.match(value)
        if match:
            year, month, day, hour, minute, second, fraction, offset_h, offset_m = match.groups()
            microsecond = int(fraction) * 100000 if fraction else 0
            if offset_h:
                tz = timezone(timedelta(hours=int(offset_h), minutes=int(offset_m)))
            else:
                tz = timezone.utc

            return datetime(int(year), int(month), int(day), int(hour), int(minute), int(second),
                            microsecond, tz)
        else:
            raise CBORDecodeError('invalid datetime string: {}'.format(value))

    def decode_epoch_datetime(self, value, fp, shareable_index=None):
        # Semantic tag 1
        return datetime.fromtimestamp(value, timezone.utc)

    def decode_positive_bignum(self, value, fp, shareable_index=None):
        # Semantic tag 2
        if PY2:
            return sum(ord(b) * (2 ** (exp * 8)) for exp, b in enumerate(reversed(value)))
        else:
            return sum(b * (2 ** (exp * 8)) for exp, b in enumerate(reversed(value)))

    def decode_negative_bignum(self, value, fp, shareable_index=None):
        # Semantic tag 3
        return -self.decode_positive_bignum(value, fp) - 1

    def decode_fraction(self, value, fp, shareable_index=None):
        # Semantic tag 4
        exp = Decimal(value[0])
        mantissa = Decimal(value[1])
        return mantissa * (10 ** exp)

    def decode_bigfloat(self, value, fp, shareable_index=None):
        # Semantic tag 5
        exp = Decimal(value[0])
        mantissa = Decimal(value[1])
        return mantissa * (2 ** exp)

    def decode_sharedref(self, value, fp, shareable_index=None):
        # Semantic tag 29
        try:
            shared = self.shareables[value]
        except IndexError:
            raise CBORDecodeError('shared reference %d not found' % value)

        if shared is None:
            raise CBORDecodeError('shared value %d has not been initialized' % value)
        else:
            return shared

    def decode_rational(self, value, fp, shareable_index=None):
        # Semantic tag 30
        return Fraction(*value)

    def decode_regexp(self, value, fp, shareable_index=None):
        # Semantic tag 35
        return re.compile(value)

    def decode_mime(self, value, fp, shareable_index=None):
        # Semantic tag 36
        return Parser().parsestr(value)

    def decode_uuid(self, value, fp, shareable_index=None):
        # Semantic tag 37
        return UUID(bytes=value)

    #
    # Special decoders (major tag 7)
    #

    def decode_float16(self, fp, shareable_index=None):
        # Code adapted from RFC 7049, appendix D
        def decode_single(single):
            return struct.unpack("!f", struct.pack("!I", single))[0]

        payload = struct.unpack('>H', fp.read(2))[0]
        value = (payload & 0x7fff) << 13 | (payload & 0x8000) << 16
        if payload & 0x7c00 != 0x7c00:
            return ldexp(decode_single(value), 112)

        return decode_single(value | 0x7f800000)

    def decode_float32(self, fp, shareable_index=None):
        return struct.unpack('>f', fp.read(4))[0]

    def decode_float64(self, fp, shareable_index=None):
        return struct.unpack('>d', fp.read(8))[0]

    major_decoders = {
        0: decode_uint,
        1: decode_negint,
        2: decode_bytestring,
        3: decode_string,
        4: decode_array,
        5: decode_map,
        6: decode_semantic,
        7: decode_special
    }

    special_decoders = {
        20: lambda self, fp, shareable_index=None: False,
        21: lambda self, fp, shareable_index=None: True,
        22: lambda self, fp, shareable_index=None: None,
        23: lambda self, fp, shareable_index=None: undefined,
        25: decode_float16,
        26: decode_float32,
        27: decode_float64,
        31: lambda self, fp, shareable_index=None: break_marker
    }

    default_semantic_decoders = {
        0: decode_datetime_string,
        1: decode_epoch_datetime,
        2: decode_positive_bignum,
        3: decode_negative_bignum,
        4: decode_fraction,
        5: decode_bigfloat,
        29: decode_sharedref,
        30: decode_rational,
        35: decode_regexp,
        36: decode_mime,
        37: decode_uuid
    }

    def decode(self, fp, shareable_index=None):
        """Decode the next value from the stream."""
        try:
            initial_byte = byte_as_integer(fp.read(1))
            major_type = initial_byte >> 5
            subtype = initial_byte & 31
        except Exception as e:
            raise CBORDecodeError('error reading major type at index {}: {}'
                                  .format(fp.tell(), e))

        decoder = self.major_decoders[major_type]
        try:
            return decoder(self, subtype, fp, shareable_index)
        except CBORDecodeError:
            raise
        except Exception as e:
            raise CBORDecodeError('error decoding value at index {}: {}'.format(fp.tell(), e))


def loads(payload, **kwargs):
    """
    Deserialize an object from a bytestring.

    :param bytes payload: the bytestring to serialize
    :param kwargs: keyword arguments passed to ``CBORDecoder()``
    :return: the deserialized object

    """
    buf = BytesIO(payload)
    decoder = CBORDecoder(**kwargs)
    return decoder.decode(buf)


def load(fp, **kwargs):
    """
    Deserialize an object from an open file.

    The file object must support memory mapping.

    :param fp: the input file (any file-like object)
    :param kwargs: keyword arguments passed to ``CBORDecoder()``
    :return: the deserialized object

    """
    decoder = CBORDecoder(**kwargs)
    return decoder.decode(fp)
