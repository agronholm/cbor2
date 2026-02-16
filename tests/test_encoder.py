import re
from binascii import unhexlify
from collections import OrderedDict
from collections.abc import Set
from datetime import date, datetime, timedelta, timezone
from decimal import Decimal
from email.mime.text import MIMEText
from fractions import Fraction
from io import BytesIO
from ipaddress import (
    IPv4Address,
    IPv4Interface,
    IPv4Network,
    IPv6Address,
    IPv6Interface,
    IPv6Network,
)
from pathlib import Path
from typing import Any
from uuid import UUID

import pytest
from _pytest.fixtures import FixtureRequest, SubRequest
from cbor2 import (
    CBOREncodeError,
    CBOREncoder,
    CBORSimpleValue,
    CBORTag,
    FrozenDict,
    dump,
    dumps,
    loads,
    shareable_encoder,
    undefined,
)
from hypothesis import given

from .hypothesis_strategies import compound_types_strategy


class TestFpAttribute:
    def test_none(self) -> None:
        with pytest.raises(ValueError, match=r"fp must be a writable file-like object"):
            CBOREncoder(None)  # type: ignore[arg-type]

    def test_not_writable(self, tmp_path: Path) -> None:
        # Test for fp not being writable
        path = tmp_path.joinpath("foo.cbor")
        path.touch()
        with (
            pytest.raises(ValueError, match=r"fp must be a writable file-like object"),
            path.open("rb") as fp,
        ):
            CBOREncoder(fp)

    def test_delete(self) -> None:
        decoder = CBOREncoder(BytesIO())
        with pytest.raises(AttributeError):
            del decoder.fp


def test_default_attr() -> None:
    with BytesIO() as stream:
        encoder = CBOREncoder(stream)
        assert encoder.default is None
        with pytest.raises(TypeError):
            encoder.default = 1  # type: ignore[assignment]
        with pytest.raises(AttributeError):
            del encoder.default


def test_timezone_attr() -> None:
    with BytesIO() as stream:
        encoder = CBOREncoder(stream)
        assert encoder.timezone is None
        with pytest.raises(TypeError):
            encoder.timezone = 1  # type: ignore[assignment]
        with pytest.raises(AttributeError):
            del encoder.timezone


def test_write() -> None:
    with BytesIO() as stream:
        encoder = CBOREncoder(stream)
        encoder.write(b"foo")
        assert stream.getvalue() == b"foo"
        with pytest.raises(TypeError):
            encoder.write(1)  # type: ignore[arg-type]


def test_encode_length() -> None:
    fp = BytesIO()
    encoder = CBOREncoder(fp)

    def reset_encoder() -> None:
        nonlocal fp, encoder
        fp = BytesIO()
        encoder = CBOREncoder(fp)

    encoder.encode_length(0, 1)
    encoder.flush()
    assert fp.getvalue() == b"\x01"

    # Array of size 2
    reset_encoder()
    encoder.encode_length(4, 2)
    encoder.flush()
    assert fp.getvalue() == b"\x82"

    # Array of indefinite size
    reset_encoder()
    encoder.encode_length(4, None)
    encoder.flush()
    assert fp.getvalue() == b"\x9f"

    # Map of size 0
    reset_encoder()
    encoder.encode_length(5, 0)
    encoder.flush()
    assert fp.getvalue() == b"\xa0"

    # Map of indefinite size
    reset_encoder()
    encoder.encode_length(5, None)
    encoder.flush()
    assert fp.getvalue() == b"\xbf"

    # Indefinite container break
    reset_encoder()
    encoder.encode_break()
    encoder.flush()
    assert fp.getvalue() == b"\xff"


def test_canonical_attr() -> None:
    # Another test purely for coverage in the C variant
    with BytesIO() as stream:
        enc = CBOREncoder(stream)
        assert not enc.canonical
        enc = CBOREncoder(stream, canonical=True)
        assert enc.canonical


@pytest.mark.parametrize(
    "value, expected",
    [
        (0, "00"),
        (1, "01"),
        (10, "0a"),
        (23, "17"),
        (24, "1818"),
        (100, "1864"),
        (1000, "1903e8"),
        (1000000, "1a000f4240"),
        (1000000000000, "1b000000e8d4a51000"),
        (18446744073709551615, "1bffffffffffffffff"),
        (18446744073709551616, "c249010000000000000000"),
        (-18446744073709551616, "3bffffffffffffffff"),
        (-18446744073709551617, "c349010000000000000000"),
        (-1, "20"),
        (-10, "29"),
        (-100, "3863"),
        (-1000, "3903e7"),
    ],
)
def test_integer(value: int, expected: str) -> None:
    assert dumps(value) == unhexlify(expected)


@pytest.mark.parametrize(
    "value, expected",
    [
        (1.1, "fb3ff199999999999a"),
        (1.0e300, "fb7e37e43c8800759c"),
        (-4.1, "fbc010666666666666"),
        (float("inf"), "f97c00"),
        (float("nan"), "f97e00"),
        (float("-inf"), "f9fc00"),
    ],
)
def test_float(value: float, expected: str) -> None:
    assert dumps(value) == unhexlify(expected)


@pytest.mark.parametrize(
    "value, expected",
    [
        (b"", "40"),
        (b"\x01\x02\x03\x04", "4401020304"),
    ],
)
def test_bytestring(value: bytes, expected: str) -> None:
    assert dumps(value) == unhexlify(expected)


def test_bytearray() -> None:
    expected = unhexlify("4401020304")
    assert dumps(bytearray(b"\x01\x02\x03\x04")) == expected


@pytest.mark.parametrize(
    "value, expected",
    [
        ("", "60"),
        ("a", "6161"),
        ("IETF", "6449455446"),
        ('"\\', "62225c"),
        ("\u00fc", "62c3bc"),
        ("\u6c34", "63e6b0b4"),
    ],
)
def test_string(value: str, expected: str) -> None:
    assert dumps(value) == unhexlify(expected)


@pytest.fixture(
    params=[
        pytest.param((False, "f4"), id="false"),
        pytest.param((True, "f5"), id="true"),
        pytest.param((None, "f6"), id="null"),
        pytest.param((undefined, "f7"), id="undefined"),
    ]
)
def special_values(request: FixtureRequest) -> tuple[object, str]:
    value, expected = request.param
    if value == "undefined":
        value = undefined

    return value, expected


def test_special(special_values: tuple[object, str]) -> None:
    value, expected = special_values
    assert dumps(value) == unhexlify(expected)


@pytest.fixture(params=[(0, "e0"), (2, "e2"), (23, "f7"), (32, "f820")])
def simple_values(request: SubRequest) -> tuple[CBORSimpleValue, str]:
    value, expected = request.param
    return CBORSimpleValue(value), expected


def test_simple_value(simple_values: tuple[CBORSimpleValue, str]) -> None:
    value, expected = simple_values
    assert dumps(value) == unhexlify(expected)


def test_simple_val_as_key() -> None:
    payload = {CBORSimpleValue(99): 1}
    result = dumps(payload)
    assert result == unhexlify("A1F86301")


#
# Tests for extension tags
#


@pytest.mark.parametrize(
    "value, as_timestamp, expected",
    [
        pytest.param(
            datetime(2013, 3, 21, 20, 4, 0, tzinfo=timezone.utc),
            False,
            "c074323031332d30332d32315432303a30343a30305a",
            id="datetime/utc",
        ),
        pytest.param(
            datetime(2013, 3, 21, 20, 4, 0, 380841, tzinfo=timezone.utc),
            False,
            "c0781b323031332d30332d32315432303a30343a30302e3338303834315a",
            id="datetime+micro/utc",
        ),
        pytest.param(
            datetime(2013, 3, 21, 22, 4, 0, tzinfo=timezone(timedelta(hours=2))),
            False,
            "c07819323031332d30332d32315432323a30343a30302b30323a3030",
            id="datetime/eet",
        ),
        pytest.param(
            datetime(2013, 3, 21, 20, 4, 0),
            False,
            "c074323031332d30332d32315432303a30343a30305a",
            id="naive",
        ),
        pytest.param(
            datetime(2013, 3, 21, 20, 4, 0, tzinfo=timezone.utc),
            True,
            "c11a514b67b0",
            id="timestamp/utc",
        ),
        pytest.param(
            datetime(2013, 3, 21, 20, 4, 0, 123456, tzinfo=timezone.utc),
            True,
            "c1fb41d452d9ec07e6b4",
            id="timestamp+micro/utc",
        ),
        pytest.param(
            datetime(2013, 3, 21, 22, 4, 0, tzinfo=timezone(timedelta(hours=2))),
            True,
            "c11a514b67b0",
            id="timestamp/eet",
        ),
    ],
)
def test_datetime(value: datetime, as_timestamp: bool, expected: str) -> None:
    assert dumps(value, datetime_as_timestamp=as_timestamp, timezone=timezone.utc) == unhexlify(
        expected
    )


@pytest.mark.parametrize(
    "value, as_timestamp, expected",
    [
        pytest.param(
            date(2013, 3, 21),
            False,
            "d903ec6a323031332d30332d3231",
            id="date/string",
        ),
        pytest.param(
            date(2018, 12, 31),
            True,
            "d8641945e8",
            id="date/timestamp",
        ),
    ],
)
def test_date(value: date, as_timestamp: bool, expected: str) -> None:
    assert dumps(value, datetime_as_timestamp=as_timestamp) == unhexlify(expected)


def test_date_as_datetime() -> None:
    expected = unhexlify("c074323031332d30332d32315430303a30303a30305a")
    assert dumps(date(2013, 3, 21), timezone=timezone.utc, date_as_datetime=True) == expected


def test_naive_datetime() -> None:
    """Test that naive datetimes are gracefully rejected when no timezone has been set."""
    with pytest.raises(CBOREncodeError) as exc:
        dumps(datetime(2013, 3, 21))
        exc.match(
            "naive datetime datetime.datetime(2013, 3, 21) encountered "
            "and no default timezone has been set"
        )
        assert isinstance(exc, ValueError)


@pytest.mark.parametrize(
    "value, expected",
    [
        pytest.param(Decimal("14.123"), "c4822219372b", id="normal"),
        pytest.param(Decimal("-14.123"), "C4822239372A", id="negative"),
        pytest.param(Decimal("NaN"), "f97e00", id="nan"),
        pytest.param(Decimal("Infinity"), "f97c00", id="inf"),
        pytest.param(Decimal("-Infinity"), "f9fc00", id="neginf"),
    ],
)
def test_decimal(value: Decimal, expected: str) -> None:
    assert dumps(value) == unhexlify(expected)


@pytest.mark.parametrize(
    "value, expected",
    [
        (3.1 + 2.1j, "d9a7f882fb4008cccccccccccdfb4000cccccccccccd"),
        (1.0e300j, "d9a7f882fb0000000000000000fb7e37e43c8800759c"),
        (0.0j, "d9a7f882fb0000000000000000fb0000000000000000"),
        (complex(float("inf"), float("inf")), "d9a7f882f97c00f97c00"),
        (complex(float("inf"), 0.0), "d9a7f882f97c00fb0000000000000000"),
        (complex(float("nan"), float("inf")), "d9a7f882f97e00f97c00"),
    ],
)
def test_complex(value: complex, expected: str) -> None:
    assert dumps(value) == unhexlify(expected)


def test_rational() -> None:
    assert dumps(Fraction(2, 5)) == unhexlify("d81e820205")


def test_regex() -> None:
    expected = unhexlify("d8236d68656c6c6f2028776f726c6429")
    assert dumps(re.compile("hello (world)")) == expected


def test_mime() -> None:
    expected = unhexlify(
        "d824787b436f6e74656e742d547970653a20746578742f706c61696e3b20636861727365743d2269736f2d38"
        "3835392d3135220a4d494d452d56657273696f6e3a20312e300a436f6e74656e742d5472616e736665722d456"
        "e636f64696e673a2071756f7465642d7072696e7461626c650a0a48656c6c6f203d413475726f"
    )
    message = MIMEText("Hello \u20acuro", "plain", "iso-8859-15")
    assert dumps(message) == expected


def test_uuid() -> None:
    expected = unhexlify("d825505eaffac8b51e480581277fdcc7842faf")
    assert dumps(UUID(hex="5eaffac8b51e480581277fdcc7842faf")) == expected


@pytest.mark.parametrize(
    "value, expected",
    [
        pytest.param(IPv4Address("192.0.2.1"), "d83444c0000201", id="ipv4addr"),
        pytest.param(IPv4Network("192.0.2.0/24"), "d83482181843c00002", id="ipv4net"),
        pytest.param(IPv4Interface("192.0.2.1/24"), "d8348244c00002011818", id="ipv4if"),
        pytest.param(
            IPv6Address("2001:0db8:1234:deed:beef:cafe:face:feed"),
            "d8365020010db81234deedbeefcafefacefeed",
            id="ipv6addr",
        ),
        pytest.param(
            IPv6Network("2001:db8:1234::/48"),
            "d8368218304620010db81234",
            id="ipv6net",
        ),
        pytest.param(
            IPv6Interface("fe80::202:2ff:ffff:fe03:303%eth0/64"),
            "d8368350fe8000000000020202fffffffe03030318404465746830",
            id="ipv6if",
        ),
    ],
)
def test_ipaddress(value: object, expected: str) -> None:
    assert dumps(value) == unhexlify(expected)


def test_custom_tag() -> None:
    expected = unhexlify("d917706548656c6c6f")
    assert dumps(CBORTag(6000, "Hello")) == expected


def test_cyclic_array() -> None:
    """Test that an array that contains itself can be serialized with value sharing enabled."""
    expected = unhexlify("d81c81d81c81d81d00")
    a: list[list[Any]] = [[]]
    a[0].append(a)
    assert dumps(a, value_sharing=True) == expected


def test_cyclic_array_nosharing() -> None:
    """Test that serializing a cyclic structure w/o value sharing will blow up gracefully."""
    a: list[list[Any]] = []
    a.append(a)
    with pytest.raises(CBOREncodeError) as exc:
        dumps(a)
        exc.match("cyclic data structure detected but value sharing is disabled")
        assert isinstance(exc, ValueError)


def test_cyclic_map() -> None:
    """Test that a dict that contains itself can be serialized with value sharing enabled."""
    expected = unhexlify("d81ca100d81d00")
    a: dict[int, dict[int, Any]] = {}
    a[0] = a
    assert dumps(a, value_sharing=True) == expected


def test_cyclic_map_nosharing() -> None:
    """Test that serializing a cyclic structure w/o value sharing will fail gracefully."""
    a: dict[int, dict[int, Any]] = {}
    a[0] = a
    with pytest.raises(CBOREncodeError) as exc:
        dumps(a)
        exc.match("cyclic data structure detected but value sharing is disabled")
        assert isinstance(exc, ValueError)


@pytest.mark.parametrize(
    "value_sharing, expected",
    [
        pytest.param(False, "828080", id="nosharing"),
        pytest.param(True, "d81c82d81c80d81d01", id="sharing"),
    ],
)
def test_not_cyclic_same_object(value_sharing: bool, expected: str) -> None:
    """Test that the same shareable object can be included twice if not in a cyclic structure."""
    a: list[Any] = []
    b = [a, a]
    assert dumps(b, value_sharing=value_sharing) == unhexlify(expected)


def test_unsupported_type() -> None:
    with pytest.raises(CBOREncodeError) as exc:
        dumps(lambda: None)
        exc.match("cannot serialize type function")
        assert isinstance(exc, TypeError)


def test_default() -> None:
    class DummyType:
        def __init__(self, state: object):
            self.state = state

    def default_encoder(encoder: CBOREncoder, value: Any) -> None:
        encoder.encode(value.state)

    expected = unhexlify("820305")
    obj = DummyType([3, 5])
    serialized = dumps(obj, default=default_encoder)
    assert serialized == expected


def test_default_cyclic() -> None:
    class DummyType:
        def __init__(self, value: object = None):
            self.value = value

    @shareable_encoder
    def default_encoder(encoder: CBOREncoder, value: Any) -> None:
        state = encoder.encode_to_bytes(value.value)
        encoder.encode(CBORTag(3000, state))

    expected = unhexlify("D81CD90BB849D81CD90BB843D81D00")
    obj = DummyType()
    obj2 = DummyType(obj)
    obj.value = obj2
    serialized = dumps(obj, value_sharing=True, default=default_encoder)
    assert serialized == expected


def test_dump_to_file(tmp_path: Path) -> None:
    path = tmp_path / "testdata.cbor"
    with path.open("wb") as fp:
        dump([1, 10], fp)

    assert path.read_bytes() == b"\x82\x01\x0a"


@pytest.mark.parametrize(
    "value, expected",
    [
        pytest.param({}, "a0", id="empty"),
        pytest.param(
            OrderedDict([(b"a", b""), (b"b", b"")]), "A2416140416240", id="bytes in order"
        ),
        pytest.param(
            OrderedDict([(b"b", b""), (b"a", b"")]), "A2416140416240", id="bytes out of order"
        ),
        pytest.param(OrderedDict([("a", ""), ("b", "")]), "a2616160616260", id="text in order"),
        pytest.param(
            OrderedDict([("b", ""), ("a", "")]), "a2616160616260", id="text out of order"
        ),
        pytest.param(
            OrderedDict([(b"00001", ""), (b"002", "")]),
            "A2433030326045303030303160",
            id="byte length",
        ),
        pytest.param(OrderedDict([(255, 0), (2, 0)]), "a2020018ff00", id="integer keys"),
        pytest.param(FrozenDict([(b"a", b""), (b"b", b"")]), "A2416140416240", id="frozendict"),
    ],
)
def test_ordered_map(value: object, expected: str) -> None:
    assert dumps(value, canonical=True) == unhexlify(expected)


@pytest.mark.parametrize(
    "value, expected",
    [
        pytest.param(3.5, "F94300", id="float 16"),
        pytest.param(100000.0, "FA47C35000", id="float 32"),
        pytest.param(3.8, "FB400E666666666666", id="float 64"),
        pytest.param(float("inf"), "f97c00", id="inf"),
        pytest.param(float("nan"), "f97e00", id="nan"),
        pytest.param(float("-inf"), "f9fc00", id="-inf"),
        pytest.param(
            float.fromhex("0x1.0p-24"),
            "f90001",
            id="float 16 minimum positive subnormal",
        ),
        pytest.param(
            float.fromhex("0x1.4p-24"),
            "fa33a00000",
            id="mantissa o/f to 32",
        ),
        pytest.param(
            float.fromhex("0x1.ff8p-63"),
            "fa207fc000",
            id="exponent o/f to 32",
        ),
        pytest.param(1e300, "fb7e37e43c8800759c", id="oversize float"),
    ],
)
def test_minimal_floats(value: float, expected: str) -> None:
    assert dumps(value, canonical=True) == unhexlify(expected)


def test_tuple_key() -> None:
    assert dumps({(2, 1): ""}) == unhexlify("a182020160")


def test_dict_key() -> None:
    assert dumps({FrozenDict({2: 1}): ""}) == unhexlify("a1a1020160")


@pytest.mark.parametrize(
    "frozen",
    [
        pytest.param(False, id="set"),
        pytest.param(True, id="frozenset"),
    ],
)
def test_set(frozen: bool) -> None:
    value: Set[str] = {"a", "b", "c"}
    if frozen:
        value = frozenset(value)

    serialized = dumps(value)
    assert len(serialized) == 10
    assert serialized.startswith(unhexlify("d9010283"))


@pytest.mark.parametrize(
    "frozen",
    [
        pytest.param(False, id="set"),
        pytest.param(True, id="frozenset"),
    ],
)
def test_canonical_set(frozen: bool) -> None:
    value: Set[str] = {"y", "x", "aa", "a"}
    if frozen:
        value = frozenset(value)

    serialized = dumps(value, canonical=True)
    assert serialized == unhexlify("d9010284616161786179626161")


@pytest.mark.parametrize(
    "value",
    [
        pytest.param("", id="empty string"),
        pytest.param("a", id="short string"),
        pytest.param("abcde", id="long string"),
        pytest.param(b"\x01\x02\x03\x04", id="bytestring"),
        pytest.param(["a", "bb", "a", "bb"], id="array of short strings"),
        pytest.param(["a", "bb", "ccc", "dddd", "a", "bb"], id="no repeated long strings"),
        pytest.param(
            {"a": "m", "bb": "nn", "e": "m", "ff": "nn"}, id="dict with short keys and strings"
        ),
        pytest.param(
            {"a": "m", "bb": "nn", "ccc": "ooo", "dddd": "pppp", "e": "m", "ff": "nn"},
            id="dict with no repeated long strings",
        ),
    ],
)
def test_encode_stringrefs_unchanged(value: object) -> None:
    expected = dumps(value)
    if isinstance(value, list) or isinstance(value, dict):
        expected = b"\xd9\x01\x00" + expected

    assert dumps(value, string_referencing=True) == expected


def test_encode_stringrefs_array() -> None:
    value = ["aaaa", "aaaa", "bbbb", "aaaa", "bbbb"]
    equivalent = [
        "aaaa",
        CBORTag(25, 0),
        "bbbb",
        CBORTag(25, 0),
        CBORTag(25, 1),
    ]
    assert dumps(value, string_referencing=True) == b"\xd9\x01\x00" + dumps(equivalent)


def test_encode_stringrefs_dict() -> None:
    value = {"aaaa": "mmmm", "bbbb": "bbbb", "cccc": "aaaa", "mmmm": "aaaa"}
    expected = unhexlify(
        "d90100a46461616161646d6d6d6d6462626262d819026463636363d81900d81901d81900"
    )
    assert dumps(value, string_referencing=True, canonical=True) == expected


def test_encode_stringrefs_datetime() -> None:
    value = [datetime(2026, 1, 19, tzinfo=timezone.utc), "abc", "abc"]
    expected = unhexlify("D9010083C074323032362D30312D31395430303A30303A30305A63616263D81901")
    assert dumps(value, string_referencing=True) == expected


@pytest.mark.parametrize(
    "tag",
    [
        pytest.param(-1, id="too small"),
        pytest.param(2**64, id="too large"),
        pytest.param("f", id="wrong type"),
    ],
)
def test_invalid_tag(tag: object) -> None:
    with pytest.raises(TypeError):
        dumps(CBORTag(tag, "value"))  # type: ignore[arg-type]


def test_largest_tag() -> None:
    expected = unhexlify("dbffffffffffffffff6176")
    assert dumps(CBORTag(2**64 - 1, "v")) == expected


@given(compound_types_strategy)
def test_invariant_encode_decode(val: object) -> None:
    """
    Tests that an encode and decode is invariant (the value is the same after
    undergoing an encode and decode)
    """
    assert loads(dumps(val)) == val


def test_indefinite_containers() -> None:
    expected = b"\x82\x00\x01"
    assert dumps([0, 1]) == expected

    expected = b"\x9f\x00\x01\xff"
    assert dumps([0, 1], indefinite_containers=True) == expected
    assert dumps([0, 1], indefinite_containers=True, canonical=True) == expected

    expected = b"\xa0"
    assert dumps({}) == expected

    expected = b"\xbf\xff"
    assert dumps({}, indefinite_containers=True) == expected
    assert dumps({}, indefinite_containers=True, canonical=True) == expected


class TestEncoderReuse:
    """
    Tests for correct behavior when reusing CBOREncoder instances.
    """

    def test_encoder_reuse_resets_shared_containers(self) -> None:
        """
        Shared container tracking should be scoped to a single encode operation,
        not persist across multiple encodes on the same encoder instance.
        """
        fp = BytesIO()
        encoder = CBOREncoder(fp, value_sharing=True)
        shared_obj = ["hello"]

        # First encode: object is tracked in shared containers
        encoder.encode([shared_obj, shared_obj])

        # Second encode on new fp: should produce valid standalone CBOR
        # (not a sharedref pointing to stale first-encode data)
        encoder.fp = fp = BytesIO()
        encoder.encode(shared_obj)
        second_output = fp.getvalue()

        # The second output must be decodable on its own
        result = loads(second_output)
        assert result == ["hello"]

    def test_encode_to_bytes_resets_shared_containers(self) -> None:
        """
        encode_to_bytes should also reset shared container tracking between calls.
        """
        fp = BytesIO()
        encoder = CBOREncoder(fp, value_sharing=True)
        shared_obj = ["hello"]

        # First encode
        encoder.encode_to_bytes([shared_obj, shared_obj])

        # Second encode should produce valid standalone CBOR
        result_bytes = encoder.encode_to_bytes(shared_obj)
        result = loads(result_bytes)
        assert result == ["hello"]

    def test_encoder_hook_does_not_reset_state(self) -> None:
        """
        When a custom encoder hook calls encode(), the shared container
        tracking should be preserved (not reset mid-operation).
        """

        class Custom:
            def __init__(self, value: object):
                self.value = value

        def custom_encoder(encoder: CBOREncoder, obj: Any) -> None:
            # Hook encodes the wrapped value
            encoder.encode(obj.value)

        # Encode a Custom wrapping a list
        data = dumps(Custom(["a", "b"]), default=custom_encoder)

        # Verify the output decodes correctly
        result = loads(data)
        assert result == ["a", "b"]

        # Test nested Custom objects - hook should work recursively
        data2 = dumps(Custom(Custom(["x"])), default=custom_encoder)
        result2 = loads(data2)
        assert result2 == ["x"]
