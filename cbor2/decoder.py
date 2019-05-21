import re
import struct
from datetime import datetime, timedelta
from io import BytesIO

from .compat import timezone, range, byte_as_integer, unpack_float16
from .types import (
    CBORDecodeError, CBORTag, undefined, break_marker, CBORSimpleValue,
    FrozenDict)

timestamp_re = re.compile(r'^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)'
                          r'(?:\.(\d+))?(?:Z|([+-]\d\d):(\d\d))$')


class CBORDecoder(object):
    """
    The CBORDecoder class implements a fully featured `CBOR`_ decoder with
    several extensions for handling shared references, big integers, rational
    numbers and so on. Typically the class is not used directly, but the
    :func:`load` and :func:`loads` functions are called to indirectly construct
    and use the class.

    When the class is constructed manually, the main entry points are
    :meth:`decode` and :meth:`decode_from_bytes`.

    :param tag_hook:
        callable that takes 2 arguments: the decoder instance, and the
        :class:`CBORTag` to be decoded. This callback is invoked for any tags
        for which there is no built-in decoder. The return value is substituted
        for the :class:`CBORTag` object in the deserialized output
    :param object_hook:
        callable that takes 2 arguments: the decoder instance, and a
        dictionary. This callback is invoked for each deserialized
        :class:`dict` object. The return value is substituted for the dict in
        the deserialized output.

    .. _CBOR: https://cbor.io/
    """

    __slots__ = (
        'tag_hook', 'object_hook', '_share_index', '_shareables', '_fp_read',
        '_immutable')

    def __init__(self, fp, tag_hook=None, object_hook=None):
        self.fp = fp
        self.tag_hook = tag_hook
        self.object_hook = object_hook
        self._share_index = None
        self._shareables = []
        self._immutable = False

    @property
    def immutable(self):
        """
        Used by decoders to check if the calling context requires an immutable
        type.  Object_hook or tag_hook should raise an exception if this flag
        is set unless the result can be safely used as a dict key.
        """
        return self._immutable

    @property
    def fp(self):
        return self._fp_read.__self__

    @fp.setter
    def fp(self, value):
        try:
            if not callable(value.read):
                raise ValueError('fp.read is not callable')
        except AttributeError:
            raise ValueError('fp object has no read method')
        else:
            self._fp_read = value.read

    def set_shareable(self, value):
        """
        Set the shareable value for the last encountered shared value marker,
        if any. If the current shared index is ``None``, nothing is done.

        :param value: the shared value
        """
        if self._share_index is not None:
            self._shareables[self._share_index] = value

    def read(self, amount):
        """
        Read bytes from the data stream.

        :param int amount: the number of bytes to read
        """
        data = self._fp_read(amount)
        if len(data) < amount:
            raise CBORDecodeError(
                'premature end of stream (expected to read {} bytes, got {} '
                'instead)'.format(amount, len(data)))

        return data

    def _decode(self, immutable=False, unshared=False):
        if immutable:
            old_immutable = self._immutable
            self._immutable = True
        if unshared:
            old_index = self._share_index
            self._share_index = None
        try:
            initial_byte = byte_as_integer(self.read(1))
            major_type = initial_byte >> 5
            subtype = initial_byte & 31
            decoder = major_decoders[major_type]
            return decoder(self, subtype)
        finally:
            if immutable:
                self._immutable = old_immutable
            if unshared:
                self._share_index = old_index

    def decode(self):
        """
        Decode the next value from the stream.

        :raises CBORDecodeError: if there is any problem decoding the stream
        """
        return self._decode()

    def decode_from_bytes(self, buf):
        """
        Wrap the given bytestring as a file and call :meth:`decode` with it as
        the argument.

        This method was intended to be used from the ``tag_hook`` hook when an
        object needs to be decoded separately from the rest but while still
        taking advantage of the shared value registry.
        """
        with BytesIO(buf) as fp:
            old_fp = self.fp
            self.fp = fp
            retval = self._decode()
            self.fp = old_fp
            return retval

    def _decode_length(self, subtype, allow_indefinite=False):
        if subtype < 24:
            return subtype
        elif subtype == 24:
            return byte_as_integer(self.read(1))
        elif subtype == 25:
            return struct.unpack('>H', self.read(2))[0]
        elif subtype == 26:
            return struct.unpack('>L', self.read(4))[0]
        elif subtype == 27:
            return struct.unpack('>Q', self.read(8))[0]
        elif subtype == 31 and allow_indefinite:
            return None
        else:
            raise CBORDecodeError(
                'unknown unsigned integer subtype 0x%x' % subtype)

    def decode_uint(self, subtype):
        # Major tag 0
        return self._decode_length(subtype)

    def decode_negint(self, subtype):
        # Major tag 1
        return -self._decode_length(subtype) - 1

    def decode_bytestring(self, subtype):
        # Major tag 2
        length = self._decode_length(subtype, allow_indefinite=True)
        if length is None:
            # Indefinite length
            buf = []
            while True:
                initial_byte = byte_as_integer(self.read(1))
                if initial_byte == 0xff:
                    return b''.join(buf)
                else:
                    length = self._decode_length(initial_byte & 0x1f)
                    value = self.read(length)
                    buf.append(value)
        else:
            return self.read(length)

    def decode_string(self, subtype):
        # Major tag 3
        return self.decode_bytestring(subtype).decode('utf-8')

    def decode_array(self, subtype):
        # Major tag 4
        length = self._decode_length(subtype, allow_indefinite=True)
        if length is None:
            # Indefinite length
            items = []
            if not self._immutable:
                self.set_shareable(items)
            while True:
                value = self._decode()
                if value is break_marker:
                    break
                else:
                    items.append(value)
        else:
            items = [None] * length
            if not self._immutable:
                self.set_shareable(items)
            for index in range(length):
                items[index] = self._decode()

        if self._immutable:
            items = tuple(items)
            self.set_shareable(items)
        return items

    def decode_map(self, subtype):
        # Major tag 5
        length = self._decode_length(subtype, allow_indefinite=True)
        if length is None:
            # Indefinite length
            dictionary = {}
            self.set_shareable(dictionary)
            while True:
                key = self._decode(immutable=True, unshared=True)
                if key is break_marker:
                    break
                else:
                    dictionary[key] = self._decode(unshared=True)
        elif self._share_index is None:
            # Optimization: pre-allocate structures from length. Note this
            # cannot be done when sharing the structure as the resulting
            # structure is not the one initially allocated
            seq = [None] * length
            for index in range(length):
                key = self._decode(immutable=True, unshared=True)
                seq[index] = (key, self._decode(unshared=True))
            dictionary = dict(seq)
        else:
            dictionary = {}
            self.set_shareable(dictionary)
            for _ in range(length):
                key = self._decode(immutable=True, unshared=True)
                dictionary[key] = self._decode(unshared=True)

        if self.object_hook:
            dictionary = self.object_hook(self, dictionary)
            self.set_shareable(dictionary)
        elif self._immutable:
            dictionary = FrozenDict(dictionary)
            self.set_shareable(dictionary)
        return dictionary

    def decode_semantic(self, subtype):
        # Major tag 6
        tagnum = self._decode_length(subtype)
        semantic_decoder = semantic_decoders.get(tagnum)
        if semantic_decoder:
            return semantic_decoder(self)
        else:
            tag = CBORTag(tagnum, self._decode(unshared=True))
            if self.tag_hook:
                return self.tag_hook(self, tag)
            else:
                return tag

    def decode_special(self, subtype):
        # Simple value
        if subtype < 20:
            return CBORSimpleValue(subtype)

        # Major tag 7
        return special_decoders[subtype](self)

    #
    # Semantic decoders (major tag 6)
    #

    def decode_datetime_string(self):
        # Semantic tag 0
        value = self._decode()
        match = timestamp_re.match(value)
        if match:
            (
                year,
                month,
                day,
                hour,
                minute,
                second,
                micro,
                offset_h,
                offset_m,
            ) = match.groups()
            if offset_h:
                tz = timezone(timedelta(hours=int(offset_h), minutes=int(offset_m)))
            else:
                tz = timezone.utc

            return datetime(
                int(year), int(month), int(day),
                int(hour), int(minute), int(second), int(micro or 0), tz)
        else:
            raise CBORDecodeError('invalid datetime string: {!r}'.format(value))

    def decode_epoch_datetime(self):
        # Semantic tag 1
        value = self._decode()
        return datetime.fromtimestamp(value, timezone.utc)

    def decode_positive_bignum(self):
        # Semantic tag 2
        from binascii import hexlify
        value = self._decode()
        return int(hexlify(value), 16)

    def decode_negative_bignum(self):
        # Semantic tag 3
        return -self.decode_positive_bignum() - 1

    def decode_fraction(self):
        # Semantic tag 4
        from decimal import Decimal
        exp, sig = self._decode()
        return Decimal(sig) * (10 ** Decimal(exp))

    def decode_bigfloat(self):
        # Semantic tag 5
        from decimal import Decimal
        exp, sig = self._decode()
        return Decimal(sig) * (2 ** Decimal(exp))

    def decode_shareable(self):
        # Semantic tag 28
        old_index = self._share_index
        self._share_index = len(self._shareables)
        self._shareables.append(None)
        try:
            return self._decode()
        finally:
            self._share_index = old_index

    def decode_sharedref(self):
        # Semantic tag 29
        value = self._decode()
        try:
            shared = self._shareables[value]
        except IndexError:
            raise CBORDecodeError('shared reference %d not found' % value)

        if shared is None:
            raise CBORDecodeError('shared value %d has not been initialized' % value)
        else:
            return shared

    def decode_rational(self):
        # Semantic tag 30
        from fractions import Fraction
        return Fraction(*self._decode())

    def decode_regexp(self):
        # Semantic tag 35
        return re.compile(self._decode())

    def decode_mime(self):
        # Semantic tag 36
        from email.parser import Parser
        return Parser().parsestr(self._decode())

    def decode_uuid(self):
        # Semantic tag 37
        from uuid import UUID
        return UUID(bytes=self._decode())

    def decode_set(self):
        # Semantic tag 258
        if self._immutable:
            return frozenset(self._decode(immutable=True))
        else:
            return set(self._decode(immutable=True))

    #
    # Special decoders (major tag 7)
    #

    def decode_simple_value(self):
        return CBORSimpleValue(byte_as_integer(self.read(1)))

    def decode_float16(self):
        payload = self.read(2)
        try:
            value = struct.unpack('>e', payload)[0]
        except struct.error:
            value = unpack_float16(payload)
        return value

    def decode_float32(self):
        return struct.unpack('>f', self.read(4))[0]

    def decode_float64(self):
        return struct.unpack('>d', self.read(8))[0]


major_decoders = {
    0: CBORDecoder.decode_uint,
    1: CBORDecoder.decode_negint,
    2: CBORDecoder.decode_bytestring,
    3: CBORDecoder.decode_string,
    4: CBORDecoder.decode_array,
    5: CBORDecoder.decode_map,
    6: CBORDecoder.decode_semantic,
    7: CBORDecoder.decode_special
}

special_decoders = {
    20: lambda self: False,
    21: lambda self: True,
    22: lambda self: None,
    23: lambda self: undefined,
    24: CBORDecoder.decode_simple_value,
    25: CBORDecoder.decode_float16,
    26: CBORDecoder.decode_float32,
    27: CBORDecoder.decode_float64,
    31: lambda self: break_marker
}

semantic_decoders = {
    0:   CBORDecoder.decode_datetime_string,
    1:   CBORDecoder.decode_epoch_datetime,
    2:   CBORDecoder.decode_positive_bignum,
    3:   CBORDecoder.decode_negative_bignum,
    4:   CBORDecoder.decode_fraction,
    5:   CBORDecoder.decode_bigfloat,
    28:  CBORDecoder.decode_shareable,
    29:  CBORDecoder.decode_sharedref,
    30:  CBORDecoder.decode_rational,
    35:  CBORDecoder.decode_regexp,
    36:  CBORDecoder.decode_mime,
    37:  CBORDecoder.decode_uuid,
    258: CBORDecoder.decode_set
}


def loads(payload, **kwargs):
    """
    Deserialize an object from a bytestring.

    :param bytes payload:
        the bytestring to deserialize
    :param kwargs:
        keyword arguments passed to :class:`CBORDecoder`
    :return:
        the deserialized object
    """
    with BytesIO(payload) as fp:
        return CBORDecoder(fp, **kwargs).decode()


def load(fp, **kwargs):
    """
    Deserialize an object from an open file.

    :param fp:
        the input file (any file-like object)
    :param kwargs:
        keyword arguments passed to :class:`CBORDecoder`
    :return:
        the deserialized object
    """
    return CBORDecoder(fp, **kwargs).decode()
