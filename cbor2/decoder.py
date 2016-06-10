import mmap
import re
import struct
from contextlib import closing
from datetime import datetime, timedelta
from decimal import Decimal
from email.parser import Parser
from fractions import Fraction
from math import ldexp
from uuid import UUID

from cbor2.compat import timezone, xrange, get_byteval, PY2
from cbor2.types import CBORTag

timestamp_re = re.compile(r'^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)'
                          r'(?:\.(\d+))?(?:Z|([+-]\d\d):(\d\d))$')


class CBORDecodeError(Exception):
    """Raised when an error occurs deserializing a CBOR datastream."""


class CBORDecoder(object):
    """
    Deserializes objects from a bytestring.

    :param Dict[int, Callable] semantic_decoders: a mapping of semantic tag -> decoder callable.
        The callable receives two arguments: the CBORDecoder instance and the tagged value.
        The callable's return value should be the transformed value.
    """

    def __init__(self, payload, semantic_decoders=None):
        self.payload = payload
        self.index = 0
        self.shareables = []
        self.mark_next_shareable = True

        if semantic_decoders:
            self.semantic_decoders = self.default_semantic_decoders.copy()
            self.semantic_decoders.update(semantic_decoders)
        else:
            self.semantic_decoders = self.default_semantic_decoders

    def decode_uint(self):
        # Major tag 0
        subtype = get_byteval(self.payload, self.index) & 31
        if subtype < 24:
            self.index += 1
            value = subtype
        elif subtype == 24:
            value = struct.unpack_from('>B', self.payload, self.index + 1)[0]
            self.index += 2
        elif subtype == 25:
            value = struct.unpack_from('>H', self.payload, self.index + 1)[0]
            self.index += 3
        elif subtype == 26:
            value = struct.unpack_from('>L', self.payload, self.index + 1)[0]
            self.index += 5
        elif subtype == 27:
            value = struct.unpack_from('>Q', self.payload, self.index + 1)[0]
            self.index += 9
        else:
            raise CBORDecodeError('unknown unsigned integer subtype 0x%x' % subtype)

        return value

    def decode_negint(self):
        # Major tag 1
        uint = self.decode_uint()
        return -uint - 1

    def decode_bytestring(self):
        # Major tag 2
        if get_byteval(self.payload, self.index) & 31 == 31:
            # Indefinite length
            self.index += 1
            buf = bytearray()
            while get_byteval(self.payload, self.index) != 0xff:
                value = self.decode_bytestring()
                buf.extend(value)

            self.index += 1
            return buf
        else:
            length = self.decode_uint()
            value = self.payload[self.index:self.index + length]
            self.index += length
            return value

    def decode_string(self):
        # Major tag 3
        if get_byteval(self.payload, self.index) & 31 == 31:
            # Indefinite length
            self.index += 1
            buf = bytearray()
            while get_byteval(self.payload, self.index) != 0xff:
                value = self.decode_bytestring()
                buf.extend(value)

            self.index += 1
            return buf.decode('utf-8')
        else:
            length = self.decode_uint()
            value = self.payload[self.index:self.index + length].decode('utf-8')
            self.index += length
            return value

    def decode_array(self):
        # Major tag 4
        items = []
        if self.mark_next_shareable:
            self.shareables.append(items)
            self.mark_next_shareable = False

        if get_byteval(self.payload, self.index) & 31 == 31:
            # Indefinite length
            self.index += 1
            while get_byteval(self.payload, self.index) != 0xff:
                value = self.decode()
                items.append(value)

            self.index += 1
        else:
            length = self.decode_uint()
            for _ in xrange(length):
                item = self.decode()
                items.append(item)

        return items

    def decode_map(self):
        # Major tag 5
        dictionary = {}
        if self.mark_next_shareable:
            self.shareables.append(dictionary)
            self.mark_next_shareable = False

        if get_byteval(self.payload, self.index) & 31 == 31:
            # Indefinite length
            self.index += 1
            while get_byteval(self.payload, self.index) != 0xff:
                key = self.decode()
                value = self.decode()
                dictionary[key] = value

            self.index += 1
        else:
            length = self.decode_uint()
            for _ in xrange(length):
                key = self.decode()
                value = self.decode()
                dictionary[key] = value

        return dictionary

    def decode_semantic(self):
        # Major tag 6
        tag = self.decode_uint()

        # Special handling for the "shareable" tag
        if tag == 28:
            self.mark_next_shareable = True
            return self.decode()

        value = self.decode()
        try:
            decoder = self.semantic_decoders[tag]
        except KeyError:
            # No special handling available
            return CBORTag(tag, value)

        return decoder(self, value)

    def decode_special(self):
        # Major tag 7
        subtype = get_byteval(self.payload, self.index) & 31
        self.index += 1
        return self.special_decoders[subtype](self)

    #
    # Semantic decoders (major tag 6)
    #

    def decode_datetime_string(self, value):
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

    def decode_epoch_datetime(self, value):
        # Semantic tag 1
        return datetime.fromtimestamp(value, timezone.utc)

    def decode_positive_bignum(self, value):
        # Semantic tag 2
        if PY2:
            return sum(ord(b) * (2 ** (exp * 8)) for exp, b in enumerate(reversed(value)))
        else:
            return sum(b * (2 ** (exp * 8)) for exp, b in enumerate(reversed(value)))

    def decode_negative_bignum(self, value):
        # Semantic tag 3
        return -self.decode_positive_bignum(value) - 1

    def decode_fraction(self, value):
        # Semantic tag 4
        exp = Decimal(value[0])
        mantissa = Decimal(value[1])
        return mantissa * (10 ** exp)

    def decode_bigfloat(self, value):
        # Semantic tag 5
        exp = Decimal(value[0])
        mantissa = Decimal(value[1])
        return mantissa * (2 ** exp)

    def decode_sharedref(self, value):
        # Semantic tag 29
        try:
            return self.shareables[value]
        except IndexError:
            raise CBORDecodeError('shared reference %d not found' % value)

    def decode_rational(self, value):
        # Semantic tag 30
        return Fraction(*value)

    def decode_regexp(self, value):
        # Semantic tag 35
        return re.compile(value)

    def decode_mime(self, value):
        # Semantic tag 36
        return Parser().parsestr(value)

    def decode_uuid(self, value):
        # Semantic tag 37
        return UUID(bytes=value)

    #
    # Special decoders (major tag 7)
    #

    def decode_float16(self):
        # Code adapted from RFC 7049, appendix D
        def decode_single(single):
            return struct.unpack("!f", struct.pack("!I", single))[0]

        payload = struct.unpack_from('>H', self.payload, self.index)[0]
        value = (payload & 0x7fff) << 13 | (payload & 0x8000) << 16
        self.index += 2
        if payload & 0x7c00 != 0x7c00:
            return ldexp(decode_single(value), 112)

        return decode_single(value | 0x7f800000)

    def decode_float32(self):
        value = struct.unpack_from('>f', self.payload, self.index)[0]
        self.index += 2
        return value

    def decode_float64(self):
        value = struct.unpack_from('>d', self.payload, self.index)[0]
        self.index += 8
        return value

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
        20: lambda self: False,
        21: lambda self: True,
        22: lambda self: None,
        23: lambda self: None,
        25: decode_float16,
        26: decode_float32,
        27: decode_float64,
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

    def decode(self):
        try:
            major_type = (get_byteval(self.payload, self.index) & 224) >> 5
        except Exception as e:
            raise CBORDecodeError('error reading major type at index {}: {}'
                                  .format(self.index, e))

        decoder = self.major_decoders[major_type]
        try:
            return decoder(self)
        except CBORDecodeError:
            raise
        except Exception as e:
            raise CBORDecodeError('error decoding value at index {}: {}'.format(self.index, e))


def loads(payload, **kwargs):
    """
    Deserialize an object from a bytestring.

    :param bytes payload: the bytestring to serialize
    :param kwargs: keyword arguments passed to ``CBORDecoder()``
    :return: the deserialized object

    """
    decoder = CBORDecoder(payload, **kwargs)
    return decoder.decode()


def load(fp, **kwargs):
    """
    Deserialize an object from an open file.

    The file object must support memory mapping.

    :param BinaryIO fp: the file object
    :param kwargs: keyword arguments passed to ``CBORDecoder()``
    :return: the deserialized object

    """
    with closing(mmap.mmap(fp.fileno(), 0, access=mmap.ACCESS_READ)) as payload:
        return loads(payload, **kwargs)
