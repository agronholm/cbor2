import sys


if sys.version_info.major < 3:
    from datetime import tzinfo, timedelta

    class timezone(tzinfo):
        def __init__(self, offset):
            self.offset = offset

        def utcoffset(self, dt):
            return self.offset

        def dst(self, dt):
            return timedelta(0)

        def tzname(self, dt):
            return 'UTC+00:00'

    def as_unicode(string):
        return string.decode('utf-8')

    def iteritems(self):
        return self.iteritems()

    def bytes_from_list(values):
        return bytes(bytearray(values))

    byte_as_integer = ord
    timezone.utc = timezone(timedelta(0))
    xrange = xrange  # noqa
    long = long  # noqa
    unicode = unicode  # noqa
else:
    from datetime import timezone

    def byte_as_integer(bytestr):
        return bytestr[0]

    def as_unicode(string):
        return string

    def iteritems(self):
        return self.items()

    xrange = range  # noqa
    long = int  # noqa
    unicode = str  # noqa
    bytes_from_list = bytes

if sys.version_info.major >= 3 and sys.version_info.minor >= 6:
    # Python 3.6 added 16 bit floating point to struct
    import struct

    def pack_float16(value):
        try:
            packed = struct.pack('>Be', 0xf9, value)
        except OverflowError:
            packed = False
        return packed

    def unpack_float16(payload):
        return struct.unpack('>e', payload)[0]


else:
    try:
        from numpy import float16, dtype, frombuffer

        big_e_float16 = dtype(float16).newbyteorder('>')

        def pack_float16(value):
            return b'\xf9' + float16(value).byteswap().tobytes()

        def unpack_float16(payload):
            return frombuffer(payload, dtype=big_e_float16)[0]

    except ImportError:
        from math import ldexp
        import struct

        def pack_float16(value):
            # Based on node-cbor by hildjj
            # which was based in turn on Carsten Borman's cn-cbor
            u32 = struct.pack('>f', value)
            u = struct.unpack('>I', u32)[0]

            if u & 0x1FFF != 0:
                return False

            s16 = (u >> 16) & 0x8000
            exponent = (u >> 23) & 0xff
            mantissa = u & 0x7fffff

            if exponent >= 113 and exponent <= 142:
                s16 += ((exponent - 112) << 10) + (mantissa >> 13)
            elif exponent >= 103 and exponent < 113:
                if mantissa & ((1 << (126 - exponent)) - 1):
                    return False

                s16 += ((mantissa + 0x800000) >> (126 - exponent))
            else:
                return False
            return struct.pack('>BH', 0xf9, s16)

        def unpack_float16(payload):
            # Code adapted from RFC 7049, appendix D
            def decode_single(single):
                return struct.unpack("!f", struct.pack("!I", single))[0]

            payload = struct.unpack('>H', payload)[0]
            value = (payload & 0x7fff) << 13 | (payload & 0x8000) << 16
            if payload & 0x7c00 != 0x7c00:
                return ldexp(decode_single(value), 112)

            return decode_single(value | 0x7f800000)
