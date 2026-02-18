from __future__ import annotations

import math
import platform
import re
import struct
import sys
from binascii import unhexlify
from datetime import date, datetime, timedelta, timezone
from decimal import Decimal
from email.message import Message
from fractions import Fraction
from io import BytesIO
from ipaddress import (
    IPv4Address,
    IPv4Interface,
    IPv4Network,
    IPv6Address,
    IPv6Interface,
    IPv6Network,
    ip_address,
    ip_interface,
    ip_network,
)
from pathlib import Path
from socket import socketpair
from typing import Any, NoReturn
from uuid import UUID

import pytest
from _pytest.fixtures import FixtureRequest
from cbor2 import (
    CBORDecodeEOF,
    CBORDecodeError,
    CBORDecoder,
    CBORDecodeValueError,
    CBORSimpleValue,
    CBORTag,
    FrozenDict,
    dumps,
    load,
    loads,
    undefined,
)

DECODER_MAX_DEPTH = 200 if platform.python_implementation() == "PyPy" else 950


@pytest.fixture
def will_overflow() -> bytes:
    """
    Construct an array/string/bytes length which would cause a memory error
    on decode. This should be less than sys.maxsize (the max integer index).
    """
    bit_size = struct.calcsize("P") * 8
    huge_length = 1 << (bit_size - 8)
    return struct.pack("Q", huge_length)


class TestFpAttribute:
    def test_none(self) -> None:
        with pytest.raises(ValueError, match=r"fp must be a readable file-like object"):
            CBORDecoder(None)  # type: ignore[arg-type]

    def test_not_readable(self, tmp_path: Path) -> None:
        # Test for fp not being readable
        with (
            pytest.raises(ValueError, match=r"fp must be a readable file-like object"),
            tmp_path.joinpath("foo.cbor").open("wb") as fp,
        ):
            CBORDecoder(fp)

    def test_delete(self) -> None:
        decoder = CBORDecoder(BytesIO())
        with pytest.raises(AttributeError):
            del decoder.fp


class TestTagHookAttribute:
    def test_callable(self) -> None:
        def tag_hook(decoder: CBORDecoder, tag: CBORTag) -> object:
            return tag.value

        decoder = CBORDecoder(BytesIO(), tag_hook=tag_hook)
        assert decoder.tag_hook is tag_hook

    def test_not_callable(self) -> None:
        with pytest.raises(TypeError, match="tag_hook must be callable or None"):
            CBORDecoder(BytesIO(), tag_hook="foo")  # type: ignore[arg-type]

    def test_delete(self) -> None:
        decoder = CBORDecoder(BytesIO())
        with pytest.raises(AttributeError):
            del decoder.tag_hook


class TestObjectHookAttribute:
    def test_success(self) -> None:
        def object_hook(decoder: CBORDecoder, value: dict[Any, Any]) -> dict[Any, Any]:
            return value

        decoder = CBORDecoder(BytesIO(), object_hook=object_hook)
        assert decoder.object_hook is object_hook

    def test_not_callable(self) -> None:
        with pytest.raises(TypeError, match="object_hook must be callable or None"):
            CBORDecoder(BytesIO(), object_hook="foo")  # type: ignore[arg-type]

    def test_delete(self) -> None:
        decoder = CBORDecoder(BytesIO())
        with pytest.raises(AttributeError):
            del decoder.object_hook


class TestStrErrorsAttribute:
    @pytest.mark.parametrize("str_errors", ["strict", "replace", "ignore"])
    def test_success(self, str_errors: str) -> None:
        decoder = CBORDecoder(BytesIO(), str_errors=str_errors)
        assert decoder.str_errors == str_errors

    def test_invalid(self) -> None:
        with pytest.raises(ValueError, match="invalid str_errors value: 'foo'"):
            CBORDecoder(BytesIO(), str_errors="foo")


def test_readonly_attributes() -> None:
    decoder = CBORDecoder(BytesIO())
    assert decoder.read_size == 4096
    assert decoder.max_depth == DECODER_MAX_DEPTH


def test_read() -> None:
    with BytesIO(b"foobar") as stream:
        decoder = CBORDecoder(stream)
        assert decoder.read(3) == b"foo"
        assert decoder.read(3) == b"bar"

        with pytest.raises(TypeError):
            decoder.read("foo")  # type: ignore[arg-type]

        with pytest.raises(CBORDecodeError):
            decoder.read(10)


def test_decode_from_bytes() -> None:
    with BytesIO(b"foobar") as stream:
        decoder = CBORDecoder(stream)
        assert decoder.decode_from_bytes(b"\x01") == 1
        with pytest.raises(TypeError):
            decoder.decode_from_bytes("foo")  # type: ignore[arg-type]


def test_stream_position_after_decode() -> None:
    """Test that the stream position is exactly at the end of the decoded CBOR value."""
    # CBOR: integer 1, followed by non-CBOR data ("extra")
    stream = BytesIO(b"\x01extra")
    assert load(stream) == 1
    # Stream position should be exactly at the end of CBOR data
    assert stream.tell() == 1
    # Should be able to read the extra data
    assert stream.read() == b"extra"


def test_non_seekable_fp() -> None:
    sock1, sock2 = socketpair()
    with sock1, sock2:
        receiver = sock2.makefile("rb")
        sock1.sendall(b"\x01\x02extra")
        decoder = CBORDecoder(receiver)
        assert decoder.decode() == 1
        assert decoder.decode() == 2
        assert receiver.read(5) == b"extra"


class TestMaximumDepth:
    def test_default(self) -> None:
        with pytest.raises(
            CBORDecodeError,
            match=f"maximum container nesting depth \\({DECODER_MAX_DEPTH}\\) exceeded",
        ):
            loads(b"\x81" * 1000 + b"\x80")

    def test_explicit(self) -> None:
        with pytest.raises(
            CBORDecodeError, match=r"maximum container nesting depth \(9\) exceeded"
        ):
            loads(b"\x81" * 10 + b"\x80", max_depth=9)


@pytest.mark.parametrize(
    "payload, expected",
    [
        ("00", 0),
        ("01", 1),
        ("0a", 10),
        ("17", 23),
        ("1818", 24),
        ("1819", 25),
        ("1864", 100),
        ("1903e8", 1000),
        ("1a000f4240", 1000000),
        ("1b000000e8d4a51000", 1000000000000),
        ("1bffffffffffffffff", 18446744073709551615),
        ("c249010000000000000000", 18446744073709551616),
        ("3bffffffffffffffff", -18446744073709551616),
        ("c349010000000000000000", -18446744073709551617),
        ("20", -1),
        ("29", -10),
        ("3863", -100),
        ("3903e7", -1000),
    ],
)
def test_integer(payload: str, expected: int) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


def test_invalid_integer_subtype() -> None:
    with pytest.raises(CBORDecodeError) as exc:
        loads(b"\x1c")
        assert str(exc.value).endswith("unknown unsigned integer subtype 0x1c")
        assert isinstance(exc, ValueError)


@pytest.mark.parametrize(
    "payload, expected",
    [
        ("f90000", 0.0),
        ("f98000", -0.0),
        ("f93c00", 1.0),
        ("fb3ff199999999999a", 1.1),
        ("f93e00", 1.5),
        ("f97bff", 65504.0),
        ("fa47c35000", 100000.0),
        ("fa7f7fffff", 3.4028234663852886e38),
        ("fb7e37e43c8800759c", 1.0e300),
        ("f90001", 5.960464477539063e-8),
        ("f90400", 0.00006103515625),
        ("f9c400", -4.0),
        ("fbc010666666666666", -4.1),
        ("f97c00", float("inf")),
        ("f9fc00", float("-inf")),
        ("fa7f800000", float("inf")),
        ("faff800000", float("-inf")),
        ("fb7ff0000000000000", float("inf")),
        ("fbfff0000000000000", float("-inf")),
    ],
)
def test_float(payload: str, expected: float) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize("payload", ["f97e00", "fa7fc00000", "fb7ff8000000000000"])
def test_float_nan(payload: str) -> None:
    decoded = loads(unhexlify(payload))
    assert math.isnan(decoded)


@pytest.fixture(
    params=[
        pytest.param(("f4", False), id="false"),
        pytest.param(("f5", True), id="true"),
        pytest.param(("f6", None), id="null"),
        pytest.param(("f7", "undefined"), id="undefined"),
    ],
)
def special_values(request: FixtureRequest) -> tuple[str, Any]:
    payload, expected = request.param
    if expected == "undefined":
        expected = undefined

    return payload, expected


def test_special(special_values: tuple[str, Any]) -> None:
    payload, expected = special_values
    decoded = loads(unhexlify(payload))
    assert decoded is expected


@pytest.mark.parametrize(
    "payload, expected",
    [
        pytest.param("40", b"", id="blank"),
        pytest.param("4401020304", b"\x01\x02\x03\x04", id="short"),
        pytest.param("5a00011170" + "12" * 70000, b"\x12" * 70000, id="long"),
    ],
)
def test_binary(payload: str, expected: bytes) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize(
    "payload, expected",
    [
        ("60", ""),
        ("6161", "a"),
        ("6449455446", "IETF"),
        ("62225c", '"\\'),
        ("62c3bc", "\u00fc"),
        ("63e6b0b4", "\u6c34"),
        pytest.param("7a00010001" + "61" * 65535 + "c3b6", "a" * 65535 + "ö", id="split_unicode"),
    ],
)
def test_string(payload: str, expected: str) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize(
    "payload",
    [
        pytest.param("6198", id="short"),
        pytest.param("7a00010000" + "61" * 65535 + "c3", id="long"),
        pytest.param("7f6198ff", id="indefinite"),
    ],
)
def test_string_invalid_utf8(payload: str) -> None:
    with pytest.raises(CBORDecodeValueError, match="error decoding text string") as exc:
        loads(unhexlify(payload))

    assert isinstance(exc.value.__cause__, UnicodeDecodeError)


def test_string_oversized() -> None:
    with pytest.raises(CBORDecodeEOF, match="premature end of stream"):
        loads(unhexlify("aeaeaeaeaeaeaeaeae0108c29843d90100d8249f0000aeaeffc26ca799"))


def test_string_issue_264_multiple_chunks_utf8_boundary() -> None:
    """Test for Issue #264: UTF-8 characters split across multiple 65536-byte chunk boundaries."""
    import struct

    # Construct: 65535 'a' + '€' (3 bytes) + 65533 'b' + '€' (3 bytes) + 100 'd'
    # Total: 131174 bytes, which spans 3 chunks (65536 + 65536 + 102)
    total_bytes = 65535 + 3 + 65533 + 3 + 100

    payload = b"\x7a" + struct.pack(">I", total_bytes)  # major type 3, 4-byte length
    payload += b"a" * 65535
    payload += "€".encode()  # U+20AC: E2 82 AC
    payload += b"b" * 65533
    payload += "€".encode()
    payload += b"d" * 100

    expected = "a" * 65535 + "€" + "b" * 65533 + "€" + "d" * 100

    result = loads(payload)
    assert result == expected
    assert len(result) == 131170  # 65535 + 1 + 65533 + 1 + 100 characters


@pytest.mark.parametrize(
    "payload, expected",
    [
        ("80", []),
        ("83010203", [1, 2, 3]),
        ("8301820203820405", [1, [2, 3], [4, 5]]),
        (
            "98190102030405060708090a0b0c0d0e0f101112131415161718181819",
            list(range(1, 26)),
        ),
    ],
)
def test_array(payload: str, expected: list[Any]) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize("payload, expected", [("a0", {}), ("a201020304", {1: 2, 3: 4})])
def test_map(payload: str, expected: dict[int, Any]) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize(
    "payload, expected",
    [
        ("a26161016162820203", {"a": 1, "b": [2, 3]}),
        ("826161a161626163", ["a", {"b": "c"}]),
        (
            "a56161614161626142616361436164614461656145",
            {"a": "A", "b": "B", "c": "C", "d": "D", "e": "E"},
        ),
    ],
)
def test_mixed_array_map(payload: str, expected: dict[str, Any]) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize(
    "payload, expected",
    [
        ("5f42010243030405ff", b"\x01\x02\x03\x04\x05"),
        ("7f657374726561646d696e67ff", "streaming"),
        ("9fff", []),
        ("9f018202039f0405ffff", [1, [2, 3], [4, 5]]),
        ("9f01820203820405ff", [1, [2, 3], [4, 5]]),
        ("83018202039f0405ff", [1, [2, 3], [4, 5]]),
        ("83019f0203ff820405", [1, [2, 3], [4, 5]]),
        (
            "9f0102030405060708090a0b0c0d0e0f101112131415161718181819ff",
            list(range(1, 26)),
        ),
        ("bf61610161629f0203ffff", {"a": 1, "b": [2, 3]}),
        ("826161bf61626163ff", ["a", {"b": "c"}]),
        ("bf6346756ef563416d7421ff", {"Fun": True, "Amt": -2}),
        ("d901029f010203ff", {1, 2, 3}),
    ],
)
def test_streaming(payload: str, expected: object) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize(
    "payload",
    [
        "5f42010200",
        "7f63737472a0",
    ],
)
def test_bad_streaming_strings(payload: str) -> None:
    with pytest.raises(
        CBORDecodeValueError,
        match=r"non-(byte|text) string \(major type \d\) found in indefinite length (byte|text) string",
    ):
        loads(unhexlify(payload))


@pytest.mark.parametrize(
    "payload, value",
    [
        ("e0", 0),
        ("e2", 2),
        ("f3", 19),
        ("f820", 32),
    ],
)
def test_simple_value(payload: str, value: int) -> None:
    wrapped = CBORSimpleValue(value)
    decoded = loads(unhexlify(payload))
    assert decoded == value
    assert decoded == wrapped


def test_simple_val_as_key() -> None:
    decoded = loads(unhexlify("A1F86301"))
    assert decoded == {CBORSimpleValue(99): 1}


#
# Tests for extension tags
#


@pytest.mark.parametrize(
    "payload, expected",
    [
        pytest.param(
            "d903ec6a323031332d30332d3231",
            date(2013, 3, 21),
            id="date/string",
        ),
        pytest.param(
            "d8641945e8",
            date(2018, 12, 31),
            id="date/timestamp",
        ),
    ],
)
def test_date(payload: str, expected: date) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize(
    "payload, expected",
    [
        pytest.param(
            "c074323031332d30332d32315432303a30343a30305a",
            datetime(2013, 3, 21, 20, 4, 0, tzinfo=timezone.utc),
            id="datetime/utc",
        ),
        pytest.param(
            "c0781b323031332d30332d32315432303a30343a30302e3338303834315a",
            datetime(2013, 3, 21, 20, 4, 0, 380841, tzinfo=timezone.utc),
            id="datetime+micro/utc",
        ),
        pytest.param(
            "c07819323031332d30332d32315432323a30343a30302b30323a3030",
            datetime(2013, 3, 21, 22, 4, 0, tzinfo=timezone(timedelta(hours=2))),
            id="datetime/eet",
        ),
        pytest.param(
            "c11a514b67b0",
            datetime(2013, 3, 21, 20, 4, 0, tzinfo=timezone.utc),
            id="timestamp/utc",
        ),
        pytest.param(
            "c11a514b67b0",
            datetime(2013, 3, 21, 22, 4, 0, tzinfo=timezone(timedelta(hours=2))),
            id="timestamp/eet",
        ),
    ],
)
def test_datetime(payload: str, expected: datetime) -> None:
    decoded = loads(unhexlify(payload))
    assert decoded == expected


@pytest.mark.parametrize(
    "payload, expected",
    [
        (
            b"\xc0\x78\x162018-08-02T07:00:59.1Z",
            datetime(2018, 8, 2, 7, 0, 59, 100000, tzinfo=timezone.utc),
        ),
        (
            b"\xc0\x78\x172018-08-02T07:00:59.01Z",
            datetime(2018, 8, 2, 7, 0, 59, 10000, tzinfo=timezone.utc),
        ),
        (
            b"\xc0\x78\x182018-08-02T07:00:59.001Z",
            datetime(2018, 8, 2, 7, 0, 59, 1000, tzinfo=timezone.utc),
        ),
        (
            b"\xc0\x78\x192018-08-02T07:00:59.0001Z",
            datetime(2018, 8, 2, 7, 0, 59, 100, tzinfo=timezone.utc),
        ),
        (
            b"\xc0\x78\x1a2018-08-02T07:00:59.00001Z",
            datetime(2018, 8, 2, 7, 0, 59, 10, tzinfo=timezone.utc),
        ),
        (
            b"\xc0\x78\x1b2018-08-02T07:00:59.000001Z",
            datetime(2018, 8, 2, 7, 0, 59, 1, tzinfo=timezone.utc),
        ),
        (
            b"\xc0\x78\x1c2018-08-02T07:00:59.0000001Z",
            datetime(2018, 8, 2, 7, 0, 59, 0, tzinfo=timezone.utc),
        ),
    ],
)
def test_datetime_secfrac(payload: bytes, expected: datetime) -> None:
    assert loads(payload) == expected


def test_datetime_secfrac_overflow() -> None:
    decoded = loads(b"\xc0\x78\x2c2018-08-02T07:00:59.100500999999999999+00:00")
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 100500, tzinfo=timezone.utc)
    decoded = loads(b"\xc0\x78\x2c2018-08-02T07:00:59.999999999999999999+00:00")
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, 999999, tzinfo=timezone.utc)


def test_datetime_invalid_string() -> None:
    with pytest.raises(CBORDecodeValueError) as excinfo:
        loads(unhexlify("c06b303030302d3132332d3031"))

    assert isinstance(excinfo.value.__cause__, ValueError)
    assert str(excinfo.value.__cause__) == "Invalid isoformat string: '0000-123-01'"


def test_datetime_overflow() -> None:
    with pytest.raises(CBORDecodeError) as excinfo:
        loads(unhexlify("c11b9b9b9b0000000000"))

    assert isinstance(excinfo.value.__cause__, OverflowError)


def test_datetime_value_too_large() -> None:
    with pytest.raises(CBORDecodeError) as excinfo:
        loads(unhexlify("c11b1616161616161616161616161616"))

    assert excinfo.value.__cause__ is not None


def test_datetime_date_out_of_range() -> None:
    with pytest.raises(CBORDecodeError) as excinfo:
        loads(unhexlify("a6c11b00002401001b000000000000ff00"))

    if platform.system() == "Windows":
        cause_exc_class: type[Exception] = OSError
    elif sys.maxsize == 2147483647:
        cause_exc_class = OverflowError
    else:
        cause_exc_class = ValueError

    assert isinstance(excinfo.value.__cause__, cause_exc_class)


def test_datetime_timezone() -> None:
    decoded = loads(b"\xc0\x78\x192018-08-02T07:00:59+00:30")
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, tzinfo=timezone(timedelta(minutes=30)))
    decoded = loads(b"\xc0\x78\x192018-08-02T07:00:59-00:30")
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, tzinfo=timezone(timedelta(minutes=-30)))
    decoded = loads(b"\xc0\x78\x192018-08-02T07:00:59+01:30")
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, tzinfo=timezone(timedelta(minutes=90)))
    decoded = loads(b"\xc0\x78\x192018-08-02T07:00:59-01:30")
    assert decoded == datetime(2018, 8, 2, 7, 0, 59, tzinfo=timezone(timedelta(minutes=-90)))


def test_positive_bignum() -> None:
    # Example from RFC 8949 section 3.4.3.
    decoded = loads(unhexlify("c249010000000000000000"))
    assert decoded == 18446744073709551616


def test_negative_bignum() -> None:
    decoded = loads(unhexlify("c349010000000000000000"))
    assert decoded == -18446744073709551617


def test_fraction() -> None:
    decoded = loads(unhexlify("c48221196ab3"))
    assert decoded == Decimal("273.15")


def test_decimal_precision() -> None:
    decoded = loads(unhexlify("c482384dc252011f1fe37d0c70ff50456ba8b891997b07d6"))
    assert decoded == Decimal("9.7703426561852468194804075821069770622934E-38")


def test_bigfloat() -> None:
    decoded = loads(unhexlify("c5822003"))
    assert decoded == Decimal("1.5")


@pytest.mark.parametrize(
    "payload, expected",
    [
        ("d9a7f882f90000f90000", 0.0j),
        ("d9a7f882fb0000000000000000fb0000000000000000", 0.0j),
        ("d9a7f882f98000f98000", -0.0j),
        ("d9a7f882f90000f93c00", 1.0j),
        ("d9a7f882fb0000000000000000fb3ff199999999999a", 1.1j),
        ("d9a7f882f93e00f93e00", 1.5 + 1.5j),
        ("d9a7f882f97bfff97bff", 65504.0 + 65504.0j),
        ("d9a7f882fa47c35000fa47c35000", 100000.0 + 100000.0j),
        ("d9a7f882f90000fb7e37e43c8800759c", 1.0e300j),
        ("d9a7f882f90000f90001", 5.960464477539063e-8j),
        ("d9a7f882f90000f90400", 0.00006103515625j),
        ("d9a7f882f90000f9c400", -4.0j),
        ("d9a7f882f90000fbc010666666666666", -4.1j),
        ("d9a7f882f90000f97c00", complex(0.0, float("inf"))),
        ("d9a7f882f97c00f90000", complex(float("inf"), 0.0)),
        ("d9a7f882f90000f9fc00", complex(0.0, float("-inf"))),
        ("d9a7f882f90000fa7f800000", complex(0.0, float("inf"))),
        ("d9a7f882f90000faff800000", complex(0.0, float("-inf"))),
        ("d9a7f882f97e00fb0000000000000000", complex(float("nan"), 0.0)),
        ("d9a7f882fb0000000000000000f97e00", complex(0.0, float("nan"))),
        ("d9a7f882f97e00f97e00", complex(float("nan"), float("nan"))),
    ],
)
def test_complex(payload: str, expected: complex) -> None:
    decoded = loads(unhexlify(payload))
    if math.isnan(expected.real):
        assert math.isnan(decoded.real)
    else:
        assert expected.real == decoded.real

    if math.isnan(expected.imag):
        assert math.isnan(decoded.imag)
    else:
        assert expected.imag == decoded.imag


def test_rational() -> None:
    decoded = loads(unhexlify("d81e820205"))
    assert decoded == Fraction(2, 5)


def test_rational_invalid_iterable() -> None:
    with pytest.raises(
        CBORDecodeValueError, match="error decoding rational: input value must be an array"
    ):
        loads(unhexlify("d81e01"))


def test_rational_zero_denominator() -> None:
    with pytest.raises(CBORDecodeValueError, match="error decoding rational") as exc:
        loads(unhexlify("d81e820100"))

    assert isinstance(exc.value.__cause__, ZeroDivisionError)


def test_regex() -> None:
    decoded = loads(unhexlify("d8236d68656c6c6f2028776f726c6429"))
    expr = re.compile("hello (world)")
    assert decoded == expr


def test_regex_unbalanced_parentheses() -> None:
    with pytest.raises(CBORDecodeValueError, match="error decoding regular expression") as exc:
        loads(unhexlify("d8236c68656c6c6f2028776f726c64"))

    assert isinstance(exc.value.__cause__, re.error)


def test_mime() -> None:
    decoded = loads(
        unhexlify(
            "d824787b436f6e74656e742d547970653a20746578742f706c61696e3b20636861727365743d2269736f"
            "2d383835392d3135220a4d494d452d56657273696f6e3a20312e300a436f6e74656e742d5472616e7366"
            "65722d456e636f64696e673a2071756f7465642d7072696e7461626c650a0a48656c6c6f203d41347572"
            "6f"
        )
    )
    assert isinstance(decoded, Message)
    assert decoded.get_payload() == "Hello =A4uro"


def test_mime_invalid_type() -> None:
    with pytest.raises(CBORDecodeValueError, match="error decoding MIME message") as exc:
        loads(unhexlify("d82401"))

    assert isinstance(exc.value.__cause__, TypeError)


def test_uuid() -> None:
    decoded = loads(unhexlify("d825505eaffac8b51e480581277fdcc7842faf"))
    assert decoded == UUID(hex="5eaffac8b51e480581277fdcc7842faf")


def test_uuid_invalid_length() -> None:
    with pytest.raises(CBORDecodeValueError, match="error decoding UUID value") as exc:
        loads(unhexlify("d8254f5eaffac8b51e480581277fdcc7842f"))

    assert isinstance(exc.value.__cause__, ValueError)


def test_uuid_invalid_type() -> None:
    with pytest.raises(CBORDecodeValueError, match="error decoding UUID value") as exc:
        loads(unhexlify("d82501"))

    assert isinstance(exc.value.__cause__, TypeError)


@pytest.mark.parametrize(
    "payload, expected",
    [
        pytest.param("d83444c0000201", IPv4Address("192.0.2.1"), id="ipv4addr"),
        pytest.param("d83482181843c00002", IPv4Network("192.0.2.0/24"), id="ipv4net"),
        pytest.param("d8348244c00002011818", IPv4Interface("192.0.2.1/24"), id="ipv4if"),
        pytest.param(
            "d8365020010db81234deedbeefcafefacefeed",
            IPv6Address("2001:0db8:1234:deed:beef:cafe:face:feed"),
            id="ipv6addr",
        ),
        pytest.param(
            "d8368218304620010db81234",
            IPv6Network("2001:db8:1234::/48"),
            id="ipv6net",
        ),
        pytest.param(
            "d8368350fe8000000000020202fffffffe03030318404465746830",
            IPv6Interface("fe80::202:2ff:ffff:fe03:303%eth0/64"),
            id="ipv6if_str_zoneid",
        ),
        pytest.param(
            "d8368350fe8000000000020202fffffffe030303184002",
            IPv6Interface("fe80::202:2ff:ffff:fe03:303%2/64"),
            id="ipv6if_num_zoneid",
        ),
    ],
)
def test_ipaddress(payload: bytes, expected: Any) -> None:
    assert loads(unhexlify(payload)) == expected


class TestDeprecatedIPAddress:
    @pytest.mark.parametrize(
        "payload, expected",
        [
            pytest.param("d9010444c00a0a01", ip_address("192.10.10.1"), id="ipv4"),
            pytest.param(
                "d901045020010db885a3000000008a2e03707334",
                ip_address("2001:db8:85a3::8a2e:370:7334"),
                id="ipv6",
            ),
            pytest.param(
                "d9010446010203040506", CBORTag(260, b"\x01\x02\x03\x04\x05\x06"), id="mac"
            ),
        ],
    )
    def test_valid(self, payload: str, expected: Any) -> None:
        assert loads(unhexlify(payload)) == expected

    @pytest.mark.parametrize("payload", ["d9010443c00a0a", "d9010401"])
    def test_invalid(self, payload: str) -> None:
        with pytest.raises(CBORDecodeError, match="invalid IP address"):
            loads(unhexlify(payload))


class TestDeprecatedIPNetwork:
    @pytest.mark.parametrize(
        "payload, expected",
        [
            pytest.param("D90105A144C0A800001818", ip_network("192.168.0.0/24"), id="ipv4_net"),
            pytest.param("d90105a144c0a800641818", ip_interface("192.168.0.100/24"), id="ipv4_if"),
            pytest.param(
                "d90105a15020010db885a3000000008a2e000000001860",
                ip_network("2001:db8:85a3:0:0:8a2e::/96"),
                id="ipv6_net",
            ),
        ],
    )
    def test_valid(self, payload: str, expected: Any) -> None:
        assert loads(unhexlify(payload)) == expected

    @pytest.mark.parametrize(
        "payload, pattern",
        [
            pytest.param(
                "d90105a244c0a80064181844c0a800001818",
                "invalid input map length for IP network: 2",
                id="length",
            ),
            pytest.param(
                "d90105a144c0a80064420102",
                r"invalid mask length for IP network: b'\\x01\\x02'",
                id="mask",
            ),
        ],
    )
    def test_invalid(self, payload: str, pattern: str) -> None:
        with pytest.raises(CBORDecodeValueError, match=pattern):
            loads(unhexlify(payload))


class TestSharedReference:
    def test_bad_reference(self) -> None:
        with pytest.raises(CBORDecodeError) as exc:
            loads(unhexlify("d81d05"))
            assert str(exc.value).endswith("shared reference 5 not found")
            assert isinstance(exc, ValueError)

    def test_uninitialized(self) -> None:
        with pytest.raises(CBORDecodeError) as exc:
            loads(unhexlify("D81CA1D81D014161"))
            assert str(exc.value).endswith("shared value 0 has not been initialized")
            assert isinstance(exc, ValueError)

    def test_immutable(self) -> None:
        # a = (1, 2, 3)
        # b = ((a, a), a)
        # data = dumps(set(b))
        decoded = loads(unhexlify("d90102d81c82d81c82d81c83010203d81d02d81d02"))
        a = [item for item in decoded if len(item) == 3][0]
        b = [item for item in decoded if len(item) == 2][0]
        assert decoded == {(a, a), a}
        assert b[0] is a
        assert b[1] is a

    def test_cyclic_array(self) -> None:
        decoded = loads(unhexlify("d81c81d81d00"))
        assert decoded == [decoded]

    def test_cyclic_map(self) -> None:
        decoded = loads(unhexlify("d81ca100d81d00"))
        assert decoded == {0: decoded}

    def test_nested_shareable_in_array(self) -> None:
        decoded = loads(unhexlify("82d81c82d81c61616162d81d00"))
        assert decoded == [["a", "b"], ["a", "b"]]
        assert decoded[0] is decoded[1]


class TestStringReference:
    def test_string_ref(self) -> None:
        decoded = loads(unhexlify("d9010085656669727374d81900667365636f6e64d81900d81901"))
        assert isinstance(decoded, list)
        assert decoded == ["first", "first", "second", "first", "second"]

    def test_ref_outside_of_namespace(self) -> None:
        with pytest.raises(CBORDecodeValueError, match="string reference outside of namespace$"):
            loads(unhexlify("85656669727374d81900667365636f6e64d81900d81901"))

    def test_invalid_string_ref(self) -> None:
        with pytest.raises(CBORDecodeValueError, match="string reference 3 not found$"):
            loads(unhexlify("d9010086656669727374d81900667365636f6e64d81900d81901d81903"))


@pytest.mark.parametrize(
    "payload, expected",
    [
        pytest.param("d9d9f71903e8", 1000, id="self_describe_cbor+int"),
        pytest.param(
            "d9d9f7c249010000000000000000",
            18446744073709551616,
            id="self_describe_cbor+positive_bignum",
        ),
    ],
)
def test_self_describe_cbor(payload: str, expected: object) -> None:
    assert loads(unhexlify(payload)) == expected


def test_unhandled_tag() -> None:
    """
    Test that a tag is simply ignored and its associated value returned if there is no special
    handling available for it.

    """
    decoded = loads(unhexlify("d917706548656c6c6f"))
    assert decoded == CBORTag(6000, "Hello")


def test_premature_end_of_stream() -> None:
    """
    Test that the decoder detects a situation where read() returned fewer than expected bytes.

    """
    with pytest.raises(CBORDecodeError) as exc:
        loads(unhexlify("437879"))
        exc.match(r"premature end of stream \(expected to read 3 bytes, got 2 instead\)")
        assert isinstance(exc, EOFError)


def test_tag_hook() -> None:
    def reverse(decoder: CBORDecoder, tag: CBORTag) -> Any:
        assert tag.tag == 6000
        return tag.value[::-1]

    decoded = loads(unhexlify("d917706548656c6c6f"), tag_hook=reverse)
    assert decoded == "olleH"


def test_tag_hook_cyclic() -> None:
    class DummyType:
        def __init__(self, value: object):
            self.value = value

    def unmarshal_dummy(decoder: CBORDecoder, tag: CBORTag) -> DummyType:
        instance = DummyType.__new__(DummyType)
        decoder.set_shareable(instance)
        instance.value = decoder.decode_from_bytes(tag.value)
        return instance

    decoded = loads(unhexlify("D81CD90BB849D81CD90BB843D81D00"), tag_hook=unmarshal_dummy)
    assert isinstance(decoded, DummyType)
    assert isinstance(decoded.value, DummyType)
    assert decoded.value.value is decoded


def test_object_hook() -> None:
    class DummyType:
        def __init__(self, state: object):
            self.state = state

    payload = unhexlify("A2616103616205")
    decoded = loads(payload, object_hook=lambda decoder, value: DummyType(value))
    assert isinstance(decoded, DummyType)
    assert decoded.state == {"a": 3, "b": 5}


def test_object_hook_exception() -> None:
    def object_hook(decoder: CBORDecoder, data: dict[Any, Any]) -> NoReturn:
        raise RuntimeError("foo")

    payload = unhexlify("A2616103616205")
    with pytest.raises(CBORDecodeError) as exc_info:
        loads(payload, object_hook=object_hook)

    assert isinstance(exc_info.value.__cause__, RuntimeError)
    assert exc_info.value.__cause__.args[0] == "foo"


def test_load_from_file(tmp_path: Path) -> None:
    path = tmp_path / "testdata.cbor"
    path.write_bytes(b"\x82\x01\x0a")
    with path.open("rb") as fp:
        obj = load(fp)

    assert obj == [1, 10]


def test_nested_dict() -> None:
    value = loads(unhexlify("A1D9177082010201"))
    assert type(value) is dict
    assert value == {CBORTag(6000, (1, 2)): 1}


def test_set() -> None:
    payload = unhexlify("d9010283616361626161")
    value = loads(payload)
    assert type(value) is set
    assert value == {"a", "b", "c"}


@pytest.mark.parametrize(
    "payload, expected",
    [
        ("a1a1616161626163", {FrozenDict({"a": "b"}): "c"}),
        (
            "A1A1A10101A1666E6573746564F5A1666E6573746564F4",
            {FrozenDict({FrozenDict({1: 1}): FrozenDict({"nested": True})}): {"nested": False}},
        ),
        ("a182010203", {(1, 2): 3}),
        ("a1d901028301020304", {frozenset({1, 2, 3}): 4}),
        ("A17f657374726561646d696e67ff01", {"streaming": 1}),
        ("d9010282d90102820102d90102820304", {frozenset({1, 2}), frozenset({3, 4})}),
    ],
)
def test_immutable_keys(payload: str, expected: object) -> None:
    value = loads(unhexlify(payload))
    assert value == expected


# Corrupted or invalid data checks


def test_huge_truncated_array(will_overflow: bytes) -> None:
    with pytest.raises(CBORDecodeError):
        loads(unhexlify("9b") + will_overflow)


def test_huge_truncated_string() -> None:
    huge_index = struct.pack("Q", sys.maxsize + 1)
    with pytest.raises((CBORDecodeError, MemoryError)):
        loads(unhexlify("7b") + huge_index + unhexlify("70717273"))


@pytest.mark.parametrize(
    "dtype_prefix", [pytest.param("7B", id="string"), pytest.param("5b", id="bytes")]
)
def test_huge_truncated_data(dtype_prefix: str, will_overflow: bytes) -> None:
    with pytest.raises((CBORDecodeError, MemoryError)):
        loads(unhexlify(dtype_prefix) + will_overflow)


@pytest.mark.parametrize(
    "tag_dtype", [pytest.param("7F7B", id="string"), pytest.param("5f5B", id="bytes")]
)
def test_huge_truncated_indefinite_data(tag_dtype: str, will_overflow: bytes) -> None:
    huge_index = struct.pack("Q", sys.maxsize + 1)
    with pytest.raises((CBORDecodeError, MemoryError)):
        loads(unhexlify(tag_dtype) + huge_index + unhexlify("70717273ff"))


@pytest.mark.parametrize(
    "data",
    [
        pytest.param("7f61777f6177ffff", id="string"),
        pytest.param("5f41775f4177ffff", id="bytes"),
    ],
)
def test_embedded_indefinite_data(data: str) -> None:
    with pytest.raises(CBORDecodeValueError):
        loads(unhexlify(data))


@pytest.mark.parametrize(
    "data", [pytest.param("7f01ff", id="string"), pytest.param("5f01ff", id="bytes")]
)
def test_invalid_indefinite_data_item(data: str) -> None:
    with pytest.raises(CBORDecodeValueError):
        loads(unhexlify(data))


@pytest.mark.parametrize(
    "data",
    [
        pytest.param("7f7bff0000000000000471717272ff", id="string"),
        pytest.param("5f5bff0000000000000471717272ff", id="bytes"),
    ],
)
def test_indefinite_overflow(data: str) -> None:
    with pytest.raises(CBORDecodeValueError):
        loads(unhexlify(data))


def test_invalid_cbor() -> None:
    with pytest.raises(CBORDecodeError):
        loads(
            unhexlify(
                "c788370016b8965bdb2074bff82e5a20e09bec21f8406e86442b87ec3ff245b70a47624dc9cdc682"
                "4b2a4c52e95ec9d6b0534b71c2b49e4bf9031500cee6869979c297bb5a8b381e98db714108415e5c"
                "50db78974c271579b01633a3ef6271be5c225eb2"
            )
        )


@pytest.mark.parametrize(
    "data, expected",
    [("fc", "1c"), ("fd", "1d"), ("fe", "1e")],
)
def test_reserved_special_tags(data: str, expected: str) -> None:
    with pytest.raises(
        CBORDecodeValueError, match=f"undefined reserved major type 7 subtype 0x{expected}"
    ):
        loads(unhexlify(data))


@pytest.mark.parametrize(
    "data, expected, typename",
    [("c400", "4", "decimal fraction"), ("c500", "5", "bigfloat")],
)
def test_decimal_payload_unpacking(data: str, expected: str, typename: str) -> None:
    with pytest.raises(
        CBORDecodeValueError, match=f"error decoding {typename}: input value must be an array"
    ):
        loads(unhexlify(data))


@pytest.mark.parametrize(
    "payload",
    [
        pytest.param(
            unhexlify("41"),
            id="bytestring",
        ),
        pytest.param(
            unhexlify("61"),
            id="unicode",
        ),
    ],
)
def test_oversized_read(payload: bytes, tmp_path: Path) -> None:
    with pytest.raises(CBORDecodeEOF, match="premature end of stream"):
        dummy_path = tmp_path / "testdata"
        dummy_path.write_bytes(payload)
        with dummy_path.open("rb") as f:
            load(f)


class TestDecoderReuse:
    """
    Tests for correct behavior when reusing CBORDecoder instances.
    """

    def test_decoder_reuse_resets_shared_refs(self) -> None:
        """
        Shared references should be scoped to a single decode operation,
        not persist across multiple decodes on the same decoder instance.
        """
        # Message with shareable tag (28)
        msg1 = dumps(CBORTag(28, "first_value"))

        # Message with sharedref tag (29) referencing index 0
        msg2 = dumps(CBORTag(29, 0))

        # Reuse decoder across messages
        decoder = CBORDecoder(BytesIO(msg1))
        result1 = decoder.decode()
        assert result1 == "first_value"

        # Second decode should fail - sharedref(0) doesn't exist in this context
        decoder.fp = BytesIO(msg2)
        with pytest.raises(CBORDecodeValueError, match="shared reference"):
            decoder.decode()

    def test_decode_from_bytes_resets_shared_refs(self) -> None:
        """
        decode_from_bytes should also reset shared references between calls.
        """
        msg1 = dumps(CBORTag(28, "value"))
        msg2 = dumps(CBORTag(29, 0))

        decoder = CBORDecoder(BytesIO(b""))
        decoder.decode_from_bytes(msg1)

        with pytest.raises(CBORDecodeValueError, match="shared reference"):
            decoder.decode_from_bytes(msg2)

    def test_shared_refs_within_single_decode(self) -> None:
        """
        Shared references must work correctly within a single decode operation.

        Note: This tests non-cyclic sibling references [shareable(x), sharedref(0)],
        which is a different pattern from test_cyclic_array/test_cyclic_map that
        test self-referencing structures like shareable([sharedref(0)]).
        """
        # [shareable("hello"), sharedref(0)] -> ["hello", "hello"]
        data = unhexlify(
            "82"  # array(2)
            "d81c"  # tag(28) shareable
            "65"  # text(5)
            "68656c6c6f"  # "hello"
            "d81d"  # tag(29) sharedref
            "00"  # unsigned(0)
        )

        result = loads(data)
        assert result == ["hello", "hello"]
        assert result[0] is result[1]  # Same object reference


def test_decode_from_bytes_in_hook_preserves_buffer() -> None:
    """Test that calling decode_from_bytes from a hook preserves stream buffer state.

    This is a documented use case from docs/customizing.rst where hooks decode
    embedded CBOR data. Before the fix, the stream's readahead buffer would be
    corrupted, causing subsequent reads to fail or return wrong data.
    """

    def tag_hook(decoder: CBORDecoder, tag: CBORTag) -> Any:
        if tag.tag == 999:
            # Decode embedded CBOR (documented pattern)
            return decoder.decode_from_bytes(tag.value)

        return tag

    # Test data: array with [tag(999, embedded_cbor), "after_hook", "final"]
    # embedded_cbor encodes: [1, 2, 3]
    data = unhexlify(
        "83"  # array(3)
        "d903e7"  # tag(999)
        "44"  # bytes(4)
        "83010203"  # embedded: array [1, 2, 3]
        "6a"  # text(10)
        "61667465725f686f6f6b"  # "after_hook"
        "65"  # text(5)
        "66696e616c"  # "final"
    )

    # Decode from stream (not bytes) to use readahead buffer
    stream = BytesIO(data)
    decoder = CBORDecoder(stream, tag_hook=tag_hook)
    result = decoder.decode()

    # Verify all values decoded correctly
    assert result == [[1, 2, 3], "after_hook", "final"]

    # First element should be the decoded embedded CBOR
    assert result[0] == [1, 2, 3]
    # Second element should be "after_hook" (not corrupted)
    assert result[1] == "after_hook"
    # Third element should be "final"
    assert result[2] == "final"


def test_decode_from_bytes_deeply_nested_in_hook() -> None:
    """Test deeply nested decode_from_bytes calls preserve buffer state.

    This tests tag(999, tag(888, tag(777, [1,2,3]))) where each tag value
    is embedded CBOR that triggers the hook recursively.

    Before the fix, even a single level would corrupt the buffer. With multiple
    levels, the buffer would be completely corrupted, mixing data from different
    BytesIO objects and the original stream.
    """

    def tag_hook(decoder: CBORDecoder, tag: CBORTag) -> Any:
        if tag.tag in [999, 888, 777]:
            # Recursively decode embedded CBOR
            return decoder.decode_from_bytes(tag.value)

        return tag

    # Test data: [tag(999, tag(888, tag(777, [1,2,3]))), "after", "final"]
    # Each tag contains embedded CBOR
    data = unhexlify(
        "83"  # array(3)
        "d903e7"  # tag(999)
        "4c"  # bytes(12)
        "d9037848d903094483010203"  # embedded: tag(888, tag(777, [1,2,3]))
        "65"  # text(5)
        "6166746572"  # "after"
        "65"  # text(5)
        "66696e616c"  # "final"
    )

    # Decode from stream to use readahead buffer
    stream = BytesIO(data)
    decoder = CBORDecoder(stream, tag_hook=tag_hook)
    result = decoder.decode()

    # With the fix: all three levels of nesting work correctly
    # Without the fix: buffer corruption at each level, test fails
    assert result == [[1, 2, 3], "after", "final"]
    assert result[0] == [1, 2, 3]
    assert result[1] == "after"
    assert result[2] == "final"


def test_str_errors_valid_utf8_unchanged() -> None:
    payload = b"\x78\x19Hello \xc3\xbcnicode \xe6\xb0\xb4 world!"
    result_strict = loads(payload, str_errors="strict")
    result_replace = loads(payload, str_errors="replace")
    assert result_strict == result_replace
    assert result_strict == "Hello \u00fcnicode \u6c34 world!"


@pytest.mark.parametrize("length", [255, 256, 257])
def test_string_stack_threshold_boundary(length: int) -> None:
    """Test stack (<=256) vs heap (>256) allocation boundary."""
    test_string = "a" * length
    if length < 24:
        payload = bytes([0x60 + length])
    elif length < 256:
        payload = b"\x78" + bytes([length])
    else:
        payload = b"\x79" + struct.pack(">H", length)

    payload += test_string.encode("utf-8")
    assert loads(payload) == test_string


def test_override_major_decoder() -> None:
    def string_decoder(decoder: CBORDecoder, subtype: int) -> str:
        return decoder.decode_string(subtype)[::-1]

    payload = unhexlify("824568656c6c6f65776f726c64")  # [b"hello", "world"]"
    assert loads(payload, major_decoders={3: string_decoder}) == [b"hello", "dlrow"]


def test_override_semantic_decoder() -> None:
    expected_datetime = datetime(2026, 2, 18)

    def date_decoder(decoder: CBORDecoder) -> datetime:
        decoder.decode_epoch_datetime()
        return datetime(2026, 2, 18)

    payload = unhexlify("c11a514b67b0")  # datetime(2013, 3, 21, 20, 4, 0, tzinfo=timezone.utc)
    assert loads(payload, semantic_decoders={1: date_decoder}) == expected_datetime
