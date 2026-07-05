from __future__ import annotations

from binascii import unhexlify
from io import BytesIO
from typing import Any

import pytest

from cbor2 import (
    CBORDecodeEOF,
    CBORDecodeError,
    CBORDecoder,
    CBORSimpleValue,
    CBORStreamDecoder,
    CBORTag,
    dumps,
    loads,
    undefined,
)
from cbor2 import tokens as t


def stream(hexdata: str) -> CBORStreamDecoder:
    return CBORStreamDecoder(BytesIO(unhexlify(hexdata)))


def test_stream_is_iterable() -> None:
    sd = stream("83010203")  # [1, 2, 3]
    assert iter(sd) is sd
    result = list(sd)
    assert result == [
        t.ArrayStart(3),
        t.Integer(1),
        t.Integer(2),
        t.Integer(3),
    ]


@pytest.mark.parametrize(
    "hexdata, expected",
    [
        ("00", t.Integer(0)),
        ("1903e8", t.Integer(1000)),
        ("20", t.Integer(-1)),
        ("3903e7", t.Integer(-1000)),
        ("f4", t.Boolean(False)),
        ("f5", t.Boolean(True)),
        ("f6", t.Null()),
        ("f7", t.Undefined()),
        ("fb3ff199999999999a", t.Float(1.1)),
        ("f0", t.Simple(16)),
        ("f8ff", t.Simple(255)),
    ],
)
def test_scalar_tokens(hexdata: str, expected: Any) -> None:
    assert stream(hexdata).decode_token() == expected


def test_string_tokens_carry_length() -> None:
    (token,) = list(stream("63666f6f"))  # "foo"
    assert isinstance(token, t.TextString)
    assert token.value == "foo"
    assert token.length == 3

    (btoken,) = list(stream("43010203"))  # b"\x01\x02\x03"
    assert isinstance(btoken, t.ByteString)
    assert btoken.value == b"\x01\x02\x03"
    assert btoken.length == 3


def test_array_and_map_start_tokens() -> None:
    assert stream("80").decode_token() == t.ArrayStart(0)
    assert stream("9f").decode_token() == t.ArrayStart(None)
    assert stream("a0").decode_token() == t.MapStart(0)
    assert stream("bf").decode_token() == t.MapStart(None)


def test_tag_token() -> None:
    tokens = list(stream("c11a514b67b0"))  # tag 1 (epoch datetime)
    assert tokens == [t.Tag(1), t.Integer(1363896240)]


def test_indefinite_text_string_streams_chunks() -> None:
    # (_ "a", "b")
    assert list(stream("7f61616162ff")) == [
        t.TextStringStart(),
        t.TextString("a"),
        t.TextString("b"),
        t.Break(),
    ]


def test_indefinite_byte_string_streams_chunks() -> None:
    # (_ h'01', h'02')
    assert list(stream("5f410141 02ff".replace(" ", ""))) == [
        t.ByteStringStart(),
        t.ByteString(b"\x01"),
        t.ByteString(b"\x02"),
        t.Break(),
    ]


def test_indefinite_array_tokens() -> None:
    assert list(stream("9f0102ff")) == [
        t.ArrayStart(None),
        t.Integer(1),
        t.Integer(2),
        t.Break(),
    ]


def test_stopiteration_at_clean_boundary() -> None:
    sd = stream("0102")
    assert sd.decode_token() == t.Integer(1)
    assert sd.decode_token() == t.Integer(2)
    with pytest.raises(StopIteration):
        next(sd)


def test_decode_token_raises_eof_at_clean_boundary() -> None:
    sd = stream("")
    with pytest.raises(CBORDecodeEOF, match="premature end of stream"):
        sd.decode_token()


def test_mid_item_eof_raises() -> None:
    # array claims 3 elements but only provides one
    sd = stream("8301")
    assert sd.decode_token() == t.ArrayStart(3)
    assert sd.decode_token() == t.Integer(1)
    with pytest.raises(CBORDecodeEOF, match="premature end of stream"):
        sd.decode_token()


def test_read_and_helpers() -> None:
    sd = CBORStreamDecoder(BytesIO(b"foobar"))
    assert sd.read(3) == b"foo"
    assert sd.read(3) == b"bar"


def test_stream_always_emits_indefinite_starts() -> None:
    # allow_indefinite is a high-level policy: the raw token stream always emits
    # the indefinite "start" tokens regardless.
    assert CBORStreamDecoder(BytesIO(unhexlify("9f"))).decode_token() == t.ArrayStart(None)
    assert CBORStreamDecoder(BytesIO(unhexlify("bf"))).decode_token() == t.MapStart(None)
    assert CBORStreamDecoder(BytesIO(unhexlify("5f"))).decode_token() == t.ByteStringStart()
    assert CBORStreamDecoder(BytesIO(unhexlify("7f"))).decode_token() == t.TextStringStart()


def test_high_level_allow_indefinite_rejects_all_starts() -> None:
    for payload in ("9f", "bf", "5f", "7f"):
        decoder = CBORDecoder(BytesIO(unhexlify(payload)), allow_indefinite=False)
        with pytest.raises(CBORDecodeError, match="encountered indefinite length"):
            decoder.decode()


def test_str_errors_forwarded() -> None:
    sd = CBORStreamDecoder(BytesIO(unhexlify("62c328")), str_errors="replace")
    token = sd.decode_token()
    assert isinstance(token, t.TextString)
    assert token.value == "\ufffd("


def test_repr_roundtrip() -> None:
    assert repr(t.Integer(5)) == "Integer(5)"
    assert repr(t.ArrayStart(None)) == "ArrayStart(None)"
    assert repr(t.MapStart(2)) == "MapStart(2)"
    assert repr(t.Tag(55799)) == "Tag(55799)"
    assert repr(t.Break()) == "Break()"
    assert repr(t.MORE) == "MORE"


def test_more_is_falsey_singleton() -> None:
    assert bool(t.MORE) is False
    assert isinstance(t.MORE, t.MoreType)


#
# Push-based assembly
#


@pytest.mark.parametrize(
    "value",
    [
        0,
        -1,
        "hello",
        b"\x01\x02",
        [1, 2, 3],
        {"a": 1, "b": [2, 3]},
        [1, [2, [3, [4]]]],
        {1: {2: {3: 4}}},
        3.14,
        True,
        None,
        undefined,
        CBORSimpleValue(200),
        CBORTag(1, 1363896240),
        [1, "a", b"b", {"c": None}],
    ],
)
def test_push_roundtrip_matches_loads(value: Any) -> None:
    data = dumps(value)
    decoder = CBORDecoder(BytesIO(data))
    result: Any = t.MORE
    for token in decoder.stream:
        result = decoder.push(token)
    assert result != t.MORE
    assert result == loads(data)


def test_push_returns_more_until_complete() -> None:
    decoder = CBORDecoder(BytesIO(dumps([1, 2])))
    tokens = list(decoder.stream)
    # Re-feed manually so we can observe intermediate MORE values.
    decoder2 = CBORDecoder(BytesIO(b""))
    outcomes = [decoder2.push(tok) for tok in tokens]
    # tokens: ArrayStart(2), Integer(1), Integer(2)
    assert outcomes[:-1] == [t.MORE, t.MORE]
    assert outcomes[-1] == [1, 2]


def test_push_multiple_top_level_items() -> None:
    buf = dumps(1) + dumps("two") + dumps([3])
    decoder = CBORDecoder(BytesIO(buf))
    results = []
    for token in decoder.stream:
        outcome = decoder.push(token)
        if outcome is not t.MORE:
            results.append(outcome)
    assert results == [1, "two", [3]]


def test_push_interception_transforms_tokens() -> None:
    decoder = CBORDecoder(BytesIO(dumps([1, 2, 3])))
    result: Any = t.MORE
    for token in decoder.stream:
        if isinstance(token, t.Integer):
            token = t.Integer(token.value * 10)
        result = decoder.push(token)
    assert result == [10, 20, 30]


def test_push_immutable() -> None:
    decoder = CBORDecoder(BytesIO(dumps([1, 2])))
    result: Any = t.MORE
    for token in decoder.stream:
        result = decoder.push(token, immutable=True)
    assert result == (1, 2)


def test_push_rejects_non_token() -> None:
    decoder = CBORDecoder(BytesIO(b""))
    with pytest.raises(ValueError, match="expected a cbor2 token object"):
        decoder.push(42)  # type: ignore[arg-type]


def test_stream_property_is_shared() -> None:
    decoder = CBORDecoder(BytesIO(dumps(1)))
    assert isinstance(decoder.stream, CBORStreamDecoder)
