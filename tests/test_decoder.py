import math
import re
from binascii import unhexlify
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from email.message import Message
from fractions import Fraction
from io import BytesIO
from ipaddress import ip_address, ip_network
from uuid import UUID

import pytest

from cbor2.types import FrozenDict


def test_fp_attr(impl):
    with pytest.raises(ValueError):
        impl.CBORDecoder(None)
    with pytest.raises(ValueError):
        class A(object):
            pass
        foo = A()
        foo.read = None
        impl.CBORDecoder(foo)
    with BytesIO(b'foobar') as stream:
        decoder = impl.CBORDecoder(stream)
        assert decoder.fp is stream
        with pytest.raises(AttributeError):
            del decoder.fp


def test_tag_hook_attr(impl):
    with BytesIO(b'foobar') as stream:
        with pytest.raises(ValueError):
            impl.CBORDecoder(stream, tag_hook='foo')
        decoder = impl.CBORDecoder(stream)
        tag_hook = lambda decoder, tag: None  # noqa: E731
        decoder.tag_hook = tag_hook
        assert decoder.tag_hook is tag_hook
        with pytest.raises(AttributeError):
            del decoder.tag_hook


def test_object_hook_attr(impl):
    with BytesIO(b'foobar') as stream:
        with pytest.raises(ValueError):
            impl.CBORDecoder(stream, object_hook='foo')
        decoder = impl.CBORDecoder(stream)
        object_hook = lambda decoder, data: None  # noqa: E731
        decoder.object_hook = object_hook
        assert decoder.object_hook is object_hook
        with pytest.raises(AttributeError):
            del decoder.object_hook


def test_str_errors_attr(impl):
    with BytesIO(b'foobar') as stream:
        with pytest.raises(ValueError):
            impl.CBORDecoder(stream, str_errors=False)
        with pytest.raises(ValueError):
            impl.CBORDecoder(stream, str_errors='foo')
        decoder = impl.CBORDecoder(stream)
        decoder.str_errors = 'replace'
        assert decoder.str_errors == 'replace'
        with pytest.raises(AttributeError):
            del decoder.str_errors


def test_read(impl):
    with BytesIO(b'foobar') as stream:
        decoder = impl.CBORDecoder(stream)
        assert decoder.read(3) == b'foo'
        assert decoder.read(3) == b'bar'
        with pytest.raises(TypeError):
            decoder.read('foo')
        with pytest.raises(impl.CBORDecodeError):
            decoder.read(10)


def test_decode_from_bytes(impl):
    with BytesIO(b'foobar') as stream:
        decoder = impl.CBORDecoder(stream)
        assert decoder.decode_from_bytes(b'\x01') == 1
        with pytest.raises(TypeError):
            decoder.decode_from_bytes(u'foo')


def test_immutable_attr(impl):
    with BytesIO(unhexlify('d917706548656c6c6f')) as stream:
        decoder = impl.CBORDecoder(stream)
        assert not decoder.immutable

        def tag_hook(decoder, tag):
            assert decoder.immutable
            return tag.value
        decoder.decode()


def test_load(impl):
    with pytest.raises(TypeError):
        impl.load()
    with pytest.raises(TypeError):
        impl.loads()
    assert impl.loads(s=b'\x01') == 1
    with BytesIO(b'\x01') as stream:
        assert impl.load(fp=stream) == 1


@pytest.mark.parametrize('payload, expected', [
    ('00', 0),
    ('01', 1),
    ('0a', 10),
    ('17', 23),
    ('1818', 24),
    ('1819', 25),
    ('1864', 100),
    ('1903e8', 1000),
    ('1a000f4240', 1000000),
    ('1b000000e8d4a51000', 1000000000000),
    ('1bffffffffffffffff', 18446744073709551615),
    ('c249010000000000000000', 18446744073709551616),
    ('3bffffffffffffffff', -18446744073709551616),
    ('c349010000000000000000', -18446744073709551617),
    ('20', -1),
    ('29', -10),
    ('3863', -100),
    ('3903e7', -1000)
])
def test_integer(impl, payload, expected):
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected


def test_invalid_integer_subtype(impl):
    with pytest.raises(impl.CBORDecodeError) as exc:
        impl.loads(b'\x1c')
        assert str(exc.value).endswith('unknown unsigned integer subtype 0x1c')
        assert isinstance(exc, ValueError)


@pytest.mark.parametrize('payload, expected', [
    ('f90000', 0.0),
    ('f98000', -0.0),
    ('f93c00', 1.0),
    ('fb3ff199999999999a', 1.1),
    ('f93e00', 1.5),
    ('f97bff', 65504.0),
    ('fa47c35000', 100000.0),
    ('fa7f7fffff', 3.4028234663852886e+38),
    ('fb7e37e43c8800759c', 1.0e+300),
    ('f90001', 5.960464477539063e-8),
    ('f90400', 0.00006103515625),
    ('f9c400', -4.0),
    ('fbc010666666666666', -4.1),
    ('f97c00', float('inf')),
    ('f9fc00', float('-inf')),
    ('fa7f800000', float('inf')),
    ('faff800000', float('-inf')),
    ('fb7ff0000000000000', float('inf')),
    ('fbfff0000000000000', float('-inf'))
])
def test_float(impl, payload, expected):
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize('payload', ['f97e00', 'fa7fc00000', 'fb7ff8000000000000'])
def test_float_nan(impl, payload):
    decoded = impl.loads(unhexlify(payload))
    assert math.isnan(decoded)


@pytest.fixture(params=[
    ('f4', False),
    ('f5', True),
    ('f6', None),
    ('f7', 'undefined')
], ids=['false', 'true', 'null', 'undefined'])
def special_values(request, impl):
    payload, expected = request.param
    if expected == 'undefined':
        expected = impl.undefined
    return payload, expected


def test_special(impl, special_values):
    payload, expected = special_values
    decoded = impl.loads(unhexlify(payload))
    assert decoded is expected


@pytest.mark.parametrize('payload, expected', [
    ('40', b''),
    ('4401020304', b'\x01\x02\x03\x04'),
])
def test_binary(impl, payload, expected):
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize('payload, expected', [
    ('60', u''),
    ('6161', u'a'),
    ('6449455446', u'IETF'),
    ('62225c', u'\"\\'),
    ('62c3bc', u'\u00fc'),
    ('63e6b0b4', u'\u6c34')
])
def test_string(impl, payload, expected):
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize('payload, expected', [
    ('80', []),
    ('83010203', [1, 2, 3]),
    ('8301820203820405', [1, [2, 3], [4, 5]]),
    ('98190102030405060708090a0b0c0d0e0f101112131415161718181819', list(range(1, 26)))
])
def test_array(impl, payload, expected):
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize('payload, expected', [
    ('a0', {}),
    ('a201020304', {1: 2, 3: 4})
])
def test_map(impl, payload, expected):
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize('payload, expected', [
    ('a26161016162820203', {'a': 1, 'b': [2, 3]}),
    ('826161a161626163', ['a', {'b': 'c'}]),
    ('a56161614161626142616361436164614461656145',
     {'a': 'A', 'b': 'B', 'c': 'C', 'd': 'D', 'e': 'E'})
])
def test_mixed_array_map(impl, payload, expected):
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize('payload, expected', [
    ('5f42010243030405ff', b'\x01\x02\x03\x04\x05'),
    ('7f657374726561646d696e67ff', 'streaming'),
    ('9fff', []),
    ('9f018202039f0405ffff', [1, [2, 3], [4, 5]]),
    ('9f01820203820405ff', [1, [2, 3], [4, 5]]),
    ('83018202039f0405ff', [1, [2, 3], [4, 5]]),
    ('83019f0203ff820405', [1, [2, 3], [4, 5]]),
    ('9f0102030405060708090a0b0c0d0e0f101112131415161718181819ff', list(range(1, 26))),
    ('bf61610161629f0203ffff', {'a': 1, 'b': [2, 3]}),
    ('826161bf61626163ff', ['a', {'b': 'c'}]),
    ('bf6346756ef563416d7421ff', {'Fun': True, 'Amt': -2}),
    ('d901029f010203ff', {1, 2, 3}),
])
def test_streaming(impl, payload, expected):
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize('payload', [
    '5f42010200',
    '7f63737472a0',
])
def test_bad_streaming_strings(impl, payload):
    with pytest.raises(impl.CBORDecodeError) as exc:
        impl.loads(unhexlify(payload))
        assert exc.match(
            r"non-(byte)?string found in indefinite length \1string")
        assert isinstance(exc, ValueError)


@pytest.fixture(params=[
    ('e0', 0),
    ('e2', 2),
    ('f3', 19),
    ('f820', 32),
])
def simple_value(request, impl):
    payload, expected = request.param
    return payload, expected, impl.CBORSimpleValue(expected)


def test_simple_value(impl, simple_value):
    payload, expected, wrapped = simple_value
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected
    assert decoded == wrapped


def test_simple_val_as_key(impl):
    decoded = impl.loads(unhexlify('A1F86301'))
    assert decoded == {impl.CBORSimpleValue(99): 1}

#
# Tests for extension tags
#


@pytest.mark.parametrize('payload, expected', [
    ('c074323031332d30332d32315432303a30343a30305a',
     datetime(2013, 3, 21, 20, 4, 0, tzinfo=timezone.utc)),
    ('c0781b323031332d30332d32315432303a30343a30302e3338303834315a',
     datetime(2013, 3, 21, 20, 4, 0, 380841, tzinfo=timezone.utc)),
    ('c07819323031332d30332d32315432323a30343a30302b30323a3030',
     datetime(2013, 3, 21, 22, 4, 0, tzinfo=timezone(timedelta(hours=2)))),
    ('c11a514b67b0', datetime(2013, 3, 21, 20, 4, 0, tzinfo=timezone.utc)),
    ('c11a514b67b0', datetime(2013, 3, 21, 22, 4, 0, tzinfo=timezone(timedelta(hours=2))))
], ids=['datetime/utc', 'datetime+micro/utc', 'datetime/eet', 'timestamp/utc', 'timestamp/eet'])
def test_datetime(impl, payload, expected):
    decoded = impl.loads(unhexlify(payload))
    assert decoded == expected


def test_datetime_secfrac(impl):
    decoded = impl.loads(b'\xc0\x78\x162018-08-02T07:00:59.1Z')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 100000, tzinfo=timezone.utc)
    decoded = impl.loads(b'\xc0\x78\x172018-08-02T07:00:59.01Z')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 10000, tzinfo=timezone.utc)
    decoded = impl.loads(b'\xc0\x78\x182018-08-02T07:00:59.001Z')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 1000, tzinfo=timezone.utc)
    decoded = impl.loads(b'\xc0\x78\x192018-08-02T07:00:59.0001Z')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 100, tzinfo=timezone.utc)
    decoded = impl.loads(b'\xc0\x78\x1a2018-08-02T07:00:59.00001Z')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 10, tzinfo=timezone.utc)
    decoded = impl.loads(b'\xc0\x78\x1b2018-08-02T07:00:59.000001Z')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 1, tzinfo=timezone.utc)
    decoded = impl.loads(b'\xc0\x78\x1c2018-08-02T07:00:59.0000001Z')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 0, tzinfo=timezone.utc)


def test_datetime_secfrac_naive_float_to_int_cast(impl):
    # A secfrac that would have rounding errors if naively parsed as
    # `int(float(secfrac) * 1000000)`.
    decoded = impl.loads(b'\xc0\x78\x202018-08-02T07:00:59.000251+00:00')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 251, tzinfo=timezone.utc)


def test_datetime_secfrac_overflow(impl):
    decoded = impl.loads(b'\xc0\x78\x2c2018-08-02T07:00:59.100500999999999999+00:00')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 100500, tzinfo=timezone.utc)
    decoded = impl.loads(b'\xc0\x78\x2c2018-08-02T07:00:59.999999999999999999+00:00')
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 999999, tzinfo=timezone.utc)


def test_datetime_secfrac_requires_digit(impl):
    with pytest.raises(impl.CBORDecodeError) as excinfo:
        impl.loads(b'\xc0\x78\x1a2018-08-02T07:00:59.+00:00')
    assert isinstance(excinfo.value, ValueError)
    assert str(excinfo.value) == "invalid datetime string: '2018-08-02T07:00:59.+00:00'"

    with pytest.raises(impl.CBORDecodeError) as excinfo:
        impl.loads(b'\xc0\x78\x152018-08-02T07:00:59.Z')
    assert isinstance(excinfo.value, ValueError)
    assert str(excinfo.value) == "invalid datetime string: '2018-08-02T07:00:59.Z'"


def test_bad_datetime(impl):
    with pytest.raises(impl.CBORDecodeError) as excinfo:
        impl.loads(unhexlify('c06b303030302d3132332d3031'))
    assert isinstance(excinfo.value, ValueError)
    assert str(excinfo.value) == "invalid datetime string: '0000-123-01'"


def test_positive_bignum(impl):
    # Example from RFC 7049 section 3.4.3.
    decoded = impl.loads(unhexlify('c249010000000000000000'))
    assert decoded == 18446744073709551616


def test_negative_bignum(impl):
    decoded = impl.loads(unhexlify('c349010000000000000000'))
    assert decoded == -18446744073709551617


def test_fraction(impl):
    decoded = impl.loads(unhexlify('c48221196ab3'))
    assert decoded == Decimal('273.15')


def test_bigfloat(impl):
    decoded = impl.loads(unhexlify('c5822003'))
    assert decoded == Decimal('1.5')


def test_rational(impl):
    decoded = impl.loads(unhexlify('d81e820205'))
    assert decoded == Fraction(2, 5)


def test_regex(impl):
    decoded = impl.loads(unhexlify('d8236d68656c6c6f2028776f726c6429'))
    expr = re.compile(u'hello (world)')
    assert decoded == expr


def test_mime(impl):
    decoded = impl.loads(unhexlify(
        'd824787b436f6e74656e742d547970653a20746578742f706c61696e3b20636861727365743d2269736f2d38'
        '3835392d3135220a4d494d452d56657273696f6e3a20312e300a436f6e74656e742d5472616e736665722d45'
        '6e636f64696e673a2071756f7465642d7072696e7461626c650a0a48656c6c6f203d413475726f'))
    assert isinstance(decoded, Message)
    assert decoded.get_payload() == 'Hello =A4uro'


def test_uuid(impl):
    decoded = impl.loads(unhexlify('d825505eaffac8b51e480581277fdcc7842faf'))
    assert decoded == UUID(hex='5eaffac8b51e480581277fdcc7842faf')


@pytest.mark.parametrize('payload, expected', [
    ('d9010444c00a0a01', ip_address(u'192.10.10.1')),
    ('d901045020010db885a3000000008a2e03707334', ip_address(u'2001:db8:85a3::8a2e:370:7334')),
    ('d9010446010203040506', (260, b'\x01\x02\x03\x04\x05\x06')),
], ids=[
    'ipv4',
    'ipv6',
    'mac',
])
def test_ipaddress(impl, payload, expected):
    if isinstance(expected, tuple):
        expected = impl.CBORTag(*expected)
    payload = unhexlify(payload)
    assert impl.loads(payload) == expected


def test_bad_ipaddress(impl):
    with pytest.raises(impl.CBORDecodeError) as exc:
        impl.loads(unhexlify('d9010443c00a0a'))
        assert str(exc.value).endswith('invalid ipaddress value %r' % b'\xc0\x0a\x0a')
        assert isinstance(exc, ValueError)
    with pytest.raises(impl.CBORDecodeError) as exc:
        impl.loads(unhexlify('d9010401'))
        assert str(exc.value).endswith('invalid ipaddress value 1')
        assert isinstance(exc, ValueError)


@pytest.mark.parametrize('payload, expected', [
    ('d90105a144c0a800641818', ip_network('192.168.0.100/24', False)),
    ('d90105a15020010db885a3000000008a2e000000001860',
     ip_network(u'2001:db8:85a3:0:0:8a2e::/96', False)),
], ids=[
    'ipv4',
    'ipv6',
])
def test_ipnetwork(impl, payload, expected):
    # XXX The following pytest.skip is only included to work-around a bug in
    # pytest under python 3.3 (which prevents the decorator above from skipping
    # correctly); remove when 3.3 support is dropped
    payload = unhexlify(payload)
    assert impl.loads(payload) == expected


def test_bad_ipnetwork(impl):
    with pytest.raises(impl.CBORDecodeError) as exc:
        impl.loads(unhexlify('d90105a244c0a80064181844c0a800001818'))
        assert str(exc.value).endswith(
            'invalid ipnetwork value %r' %
            {b'\xc0\xa8\x00d': 24, b'\xc0\xa8\x00\x00': 24})
        assert isinstance(exc, ValueError)
    with pytest.raises(impl.CBORDecodeError) as exc:
        impl.loads(unhexlify('d90105a144c0a80064420102'))
        assert str(exc.value).endswith(
            'invalid ipnetwork value %r' %
            {b'\xc0\xa8\x00d': b'\x01\x02'})
        assert isinstance(exc, ValueError)


def test_bad_shared_reference(impl):
    with pytest.raises(impl.CBORDecodeError) as exc:
        impl.loads(unhexlify('d81d05'))
        assert str(exc.value).endswith('shared reference 5 not found')
        assert isinstance(exc, ValueError)


def test_uninitialized_shared_reference(impl):
    with pytest.raises(impl.CBORDecodeError) as exc:
        impl.loads(unhexlify('D81CA1D81D014161'))
        assert str(exc.value).endswith('shared value 0 has not been initialized')
        assert isinstance(exc, ValueError)


def test_immutable_shared_reference(impl):
    # a = (1, 2, 3)
    # b = ((a, a), a)
    # data = dumps(set(b))
    decoded = impl.loads(unhexlify('d90102d81c82d81c82d81c83010203d81d02d81d02'))
    a = [item for item in decoded if len(item) == 3][0]
    b = [item for item in decoded if len(item) == 2][0]
    assert decoded == set(((a, a), a))
    assert b[0] is a
    assert b[1] is a


def test_cyclic_array(impl):
    decoded = impl.loads(unhexlify('d81c81d81d00'))
    assert decoded == [decoded]


def test_cyclic_map(impl):
    decoded = impl.loads(unhexlify('d81ca100d81d00'))
    assert decoded == {0: decoded}


@pytest.mark.parametrize('payload, expected', [
    ('d9d9f71903e8', 1000),
    ('d9d9f7c249010000000000000000', 18446744073709551616),
], ids=['self_describe_cbor+int', 'self_describe_cbor+positive_bignum'])
def test_self_describe_cbor(impl, payload, expected):
    assert impl.loads(unhexlify(payload)) == expected


def test_unhandled_tag(impl):
    """
    Test that a tag is simply ignored and its associated value returned if there is no special
    handling available for it.

    """
    decoded = impl.loads(unhexlify('d917706548656c6c6f'))
    assert decoded == impl.CBORTag(6000, u'Hello')


def test_premature_end_of_stream(impl):
    """
    Test that the decoder detects a situation where read() returned fewer than expected bytes.

    """
    with pytest.raises(impl.CBORDecodeError) as exc:
        impl.loads(unhexlify('437879'))
        exc.match(r'premature end of stream \(expected to read 3 bytes, got 2 instead\)')
        assert isinstance(exc, EOFError)


def test_tag_hook(impl):
    def reverse(decoder, tag):
        return tag.value[::-1]

    decoded = impl.loads(unhexlify('d917706548656c6c6f'), tag_hook=reverse)
    assert decoded == u'olleH'


def test_tag_hook_cyclic(impl):
    class DummyType(object):
        def __init__(self, value):
            self.value = value

    def unmarshal_dummy(decoder, tag):
        instance = DummyType.__new__(DummyType)
        decoder.set_shareable(instance)
        instance.value = decoder.decode_from_bytes(tag.value)
        return instance

    decoded = impl.loads(unhexlify('D81CD90BB849D81CD90BB843D81D00'), tag_hook=unmarshal_dummy)
    assert isinstance(decoded, DummyType)
    assert decoded.value.value is decoded


def test_object_hook(impl):
    class DummyType(object):
        def __init__(self, state):
            self.state = state

    payload = unhexlify('A2616103616205')
    decoded = impl.loads(payload, object_hook=lambda decoder, value: DummyType(value))
    assert isinstance(decoded, DummyType)
    assert decoded.state == {'a': 3, 'b': 5}


def test_load_from_file(impl, tmpdir):
    path = tmpdir.join('testdata.cbor')
    path.write_binary(b'\x82\x01\x0a')
    with path.open('rb') as fp:
        obj = impl.load(fp)

    assert obj == [1, 10]


def test_nested_exception(impl):
    with pytest.raises((impl.CBORDecodeError, TypeError)) as exc:
        impl.loads(unhexlify('A1D9177082010201'))
        exc.match(
            r"(unhashable type: '(_?cbor2\.)?CBORTag'"
            r"|"
            r"'(_?cbor2\.)?CBORTag' objects are unhashable)")
        assert isinstance(exc, TypeError)


def test_set(impl):
    payload = unhexlify('d9010283616361626161')
    value = impl.loads(payload)
    assert type(value) is set
    assert value == {u'a', u'b', u'c'}


@pytest.mark.parametrize('payload, expected', [
    ('a1a1616161626163', {FrozenDict({'a': 'b'}): 'c'}),
    ('A1A1A10101A1666E6573746564F5A1666E6573746564F4',
        {FrozenDict({FrozenDict({1: 1}): FrozenDict({"nested": True})}): {"nested": False}}),
    ('a182010203', {(1, 2): 3}),
    ('a1d901028301020304', {frozenset({1, 2, 3}): 4}),
    ('A17f657374726561646d696e67ff01', {"streaming": 1}),
    ('d9010282d90102820102d90102820304', {frozenset({1, 2}), frozenset({3, 4})})
])
def test_immutable_keys(impl, payload, expected):
    value = impl.loads(unhexlify(payload))
    assert value == expected


def test_huge_truncated_array(impl):
    with pytest.raises(impl.CBORDecodeEOF):
        impl.loads(unhexlify('9b37388519251ae9ca'))


def test_huge_truncated_bytes(impl):
    with pytest.raises((impl.CBORDecodeEOF, MemoryError)):
        impl.loads(unhexlify('5b37388519251ae9ca'))


def test_huge_truncated_string(impl):
    with pytest.raises((impl.CBORDecodeEOF, MemoryError)):
        impl.loads(unhexlify('7B37388519251ae9ca'))
