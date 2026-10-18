import binascii
import json
from datetime import date, datetime, timezone
from io import BytesIO, TextIOWrapper
from pathlib import Path

import pytest
from pytest import MonkeyPatch

import cbor2.tool


@pytest.mark.parametrize(
    "value, expected",
    [
        pytest.param((1, 2, 3), [1, 2, 3], id="tuple"),
        pytest.param({b"\x01\x02\x03": "b"}, {"\x01\x02\x03": "b"}, id="byte_key"),
        pytest.param({"dict": {"b": 17}}, {"dict": {"b": 17}}, id="recursion"),
    ],
)
def test_key_to_str(value: object, expected: object) -> None:
    assert cbor2.tool.key_to_str(value) == expected


def test_default() -> None:
    with pytest.raises(TypeError):
        json.dumps(BytesIO(b""), cls=cbor2.tool.DefaultEncoder)


@pytest.mark.parametrize(
    "payload",
    [
        pytest.param("D81CA16162D81CA16161D81D00", id="dict"),
        pytest.param("d81c81d81c830102d81d00", id="list"),
    ],
)
def test_self_referencing(payload: str) -> None:
    decoded = cbor2.loads(binascii.unhexlify(payload))
    with pytest.raises(ValueError, match="Cannot convert self-referential data to JSON"):
        cbor2.tool.key_to_str(decoded)


def test_nonrecursive_ref() -> None:
    payload = "d81c83d81ca26162d81ca16161016163d81d02d81d01d81d01"
    decoded = cbor2.loads(binascii.unhexlify(payload))
    result = cbor2.tool.key_to_str(decoded)
    expected = [
        {"b": {"a": 1}, "c": {"a": 1}},
        {"b": {"a": 1}, "c": {"a": 1}},
        {"b": {"a": 1}, "c": {"a": 1}},
    ]
    assert result == expected


def test_stdin(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    path = tmp_path / "outfile"
    argv = ["-o", str(path)]
    inbuf = TextIOWrapper(BytesIO(binascii.unhexlify("02")))
    with monkeypatch.context() as m:
        m.setattr("sys.argv", [""] + argv)
        m.setattr("sys.stdin", inbuf)
        cbor2.tool.main()
        assert path.read_text() == "2\n"


def test_stdout(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    argv = ["-o", "-"]
    inbuf = TextIOWrapper(BytesIO(binascii.unhexlify("02")))
    outbuf = BytesIO()
    with monkeypatch.context() as m:
        m.setattr("sys.argv", [""] + argv)
        m.setattr("sys.stdin", inbuf)
        m.setattr("sys.stdout", outbuf)
        cbor2.tool.main()


def test_readfrom(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    in_path = tmp_path / "infile"
    out_path = tmp_path / "outfile"
    in_path.write_bytes(binascii.unhexlify("02"))
    argv = ["-o", str(out_path), str(in_path)]
    with monkeypatch.context() as m:
        m.setattr("sys.argv", [""] + argv)
        cbor2.tool.main()
        assert out_path.read_text() == "2\n"


def test_b64(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    out_path = tmp_path / "outfile"
    argv = ["-d", "-o", str(out_path)]
    inbuf = TextIOWrapper(BytesIO(b"oQID"))
    with monkeypatch.context() as m:
        m.setattr("sys.argv", [""] + argv)
        m.setattr("sys.stdin", inbuf)
        cbor2.tool.main()
        assert out_path.read_text() == '{"2": 3}\n'


def test_stream(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    out_path = tmp_path / "outfile"
    argv = ["--sequence", "-o", str(out_path)]
    inbuf = TextIOWrapper(BytesIO(binascii.unhexlify("0203")))
    with monkeypatch.context() as m:
        m.setattr("sys.argv", [""] + argv)
        m.setattr("sys.stdin", inbuf)
        cbor2.tool.main()
        assert out_path.read_text() == "2\n3\n"


def test_embed_bytes(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    out_path = tmp_path / "outfile"
    argv = ["-o", str(out_path)]
    inbuf = TextIOWrapper(BytesIO(binascii.unhexlify("42C2C2")))
    with monkeypatch.context() as m:
        m.setattr("sys.argv", [""] + argv)
        m.setattr("sys.stdin", inbuf)
        cbor2.tool.main()
        assert out_path.read_text() == '"\\\\xc2\\\\xc2"\n'


def test_dtypes_from_file(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    in_path = Path(__file__).with_name("examples.cbor.b64")
    expected = Path(__file__).with_name("examples.json").read_text()
    out_path = tmp_path / "outfile.json"
    argv = ["--sort-keys", "--pretty", "-d", "-o", str(out_path), str(in_path)]
    monkeypatch.setattr("sys.argv", [""] + argv)
    cbor2.tool.main()
    assert out_path.read_text() == expected


def test_ignore_tag(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    out_path = tmp_path / "outfile"
    argv = ["-o", str(out_path), "-i", "6000"]
    inbuf = TextIOWrapper(BytesIO(binascii.unhexlify("D917706548656C6C6F")))
    expected = '"Hello"\n'
    with monkeypatch.context() as m:
        m.setattr("sys.argv", [""] + argv)
        m.setattr("sys.stdin", inbuf)
        cbor2.tool.main()
        assert out_path.read_text() == expected


@pytest.mark.parametrize("as_timestamp", [False, True], ids=["tag1004", "tag100"])
@pytest.mark.parametrize("nested", [False, True], ids=["scalar", "nested"])
def test_date(monkeypatch: MonkeyPatch, tmp_path: Path, as_timestamp: bool, nested: bool) -> None:
    value = date(2026, 10, 18)
    obj = {"dates": [value]} if nested else value
    in_path = tmp_path / "date.cbor"
    in_path.write_bytes(cbor2.dumps(obj, datetime_as_timestamp=as_timestamp))
    out_path = tmp_path / "date.json"
    monkeypatch.setattr("sys.argv", ["", "-o", str(out_path), str(in_path)])

    cbor2.tool.main()

    expected = {"dates": ["2026-10-18"]} if nested else "2026-10-18"
    assert json.loads(out_path.read_text()) == expected


def test_datetime() -> None:
    value = datetime(2026, 10, 18, 12, 30, tzinfo=timezone.utc)
    assert json.dumps(value, cls=cbor2.tool.DefaultEncoder) == '"2026-10-18T12:30:00+00:00"'
