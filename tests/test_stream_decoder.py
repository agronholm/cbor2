from __future__ import annotations

from binascii import unhexlify
from io import BytesIO
from typing import Any

import pytest

from cbor2 import (
    CBORDecodeEOF,
    CBORDecodeError,
    CBORDecoder,
    CBORStreamDecoder,
    dumps,
    loads,
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


def test_stream_property_is_shared() -> None:
    decoder = CBORDecoder(BytesIO(dumps(1)))
    assert isinstance(decoder.stream, CBORStreamDecoder)


#
# Token hooks (selective, native-loop customization)
#


def test_token_hook_customizes_single_leaf_type() -> None:
    data = dumps([1, 2, "foo", {"k": 3}])
    out = loads(data, token_hooks={t.Integer: lambda tok: tok.value * 2})
    # only integers are transformed; strings/maps/lists stay native
    assert out == [2, 4, "foo", {"k": 6}]


def test_token_hook_receives_token_object() -> None:
    seen = []

    def hook(tok: t.TextString) -> str:
        seen.append((type(tok), tok.value, tok.length))
        return tok.value.upper()

    assert loads(dumps(["ab", "cde"]), token_hooks={t.TextString: hook}) == ["AB", "CDE"]
    assert seen == [(t.TextString, "ab", 2), (t.TextString, "cde", 3)]


def test_token_hook_multiple_types() -> None:
    hooks = {
        t.Integer: lambda tok: tok.value + 100,
        t.Boolean: lambda tok: "yes" if tok.value else "no",
        t.Null: lambda tok: "nothing",
    }
    assert loads(dumps([1, True, None, "x"]), token_hooks=hooks) == [101, "yes", "nothing", "x"]


def test_token_hook_bytestring_with_length() -> None:
    class Blob:
        def __init__(self, data: bytes) -> None:
            self.data = data

        def __eq__(self, other: object) -> bool:
            return isinstance(other, Blob) and other.data == self.data

    out = loads(dumps(b"hello"), token_hooks={t.ByteString: lambda tok: Blob(tok.value)})
    assert out == Blob(b"hello")


def test_token_hook_on_decoder_instance() -> None:
    decoder = CBORDecoder(BytesIO(dumps(5)), token_hooks={t.Integer: lambda tok: -tok.value})
    assert decoder.decode() == -5
    assert decoder.token_hooks is not None
    assert set(decoder.token_hooks) == {t.Integer}


def test_token_hooks_getter_default_none() -> None:
    assert CBORDecoder(BytesIO(dumps(1))).token_hooks is None


def test_token_hook_rejects_non_token_type() -> None:
    with pytest.raises(TypeError, match="is not a hookable token type"):
        loads(dumps(1), token_hooks={int: lambda tok: tok})  # type: ignore[dict-item]


def test_token_hook_rejects_structural_token_type() -> None:
    # container/tag starts are not customizable via token hooks
    with pytest.raises(TypeError, match="is not a hookable token type"):
        loads(dumps([1]), token_hooks={t.ArrayStart: lambda tok: tok})


def test_token_hook_rejects_non_callable() -> None:
    with pytest.raises(TypeError, match="token_hooks values must be callable"):
        loads(dumps(1), token_hooks={t.Integer: "nope"})  # type: ignore[dict-item]


def test_token_hook_exception_is_wrapped() -> None:
    def boom(tok: t.Integer) -> Any:
        raise RuntimeError("kaboom")

    with pytest.raises(CBORDecodeError) as exc_info:
        loads(dumps(1), token_hooks={t.Integer: boom})

    assert isinstance(exc_info.value.__cause__, RuntimeError)


def test_token_hook_does_not_affect_unhooked_types() -> None:
    # A hook on floats must leave ints untouched (native path)
    out = loads(dumps([1, 2.7]), token_hooks={t.Float: lambda tok: round(tok.value)})
    assert out == [1, 3]
    assert isinstance(out[0], int) and isinstance(out[1], int)
