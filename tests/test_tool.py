import binascii
import json
import subprocess
import sys
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


@pytest.mark.parametrize("format_args", [[], ["--format=cbor"]])
def test_stdin(monkeypatch: MonkeyPatch, tmp_path: Path, format_args: list[str]) -> None:
    path = tmp_path / "outfile"
    argv = ["-o", str(path), *format_args]
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


def test_date() -> None:
    value = date(2026, 10, 18)
    assert json.dumps(value, cls=cbor2.tool.DefaultEncoder) == '"2026-10-18"'


def test_datetime() -> None:
    value = datetime(2026, 10, 18, 12, 30, tzinfo=timezone.utc)
    assert json.dumps(value, cls=cbor2.tool.DefaultEncoder) == '"2026-10-18T12:30:00+00:00"'


@pytest.mark.parametrize(
    "document, expected",
    [
        (
            '{"message": "caf\u00e9", "values": [1, true, null]}',
            b"\xa2\x67message\x65caf\xc3\xa9\x66values\x83\x01\xf5\xf6",
        ),
        ('"hello"', b"\x65hello"),
        ("0", b"\x00"),
        ("false", b"\xf4"),
        ("null", b"\xf6"),
        ("[]", b"\x80"),
        ("[10, 13, 26]", b"\x83\x0a\x0d\x18\x1a"),
    ],
)
@pytest.mark.skipif(
    sys.platform == "emscripten", reason="Subprocesses are unavailable on Emscripten"
)
def test_json_stdin(document: str, expected: bytes) -> None:
    result = subprocess.run(
        [sys.executable, "-m", "cbor2.tool", "--format=json"],
        input=document.encode("utf-8"),
        capture_output=True,
        check=True,
    )
    assert result.stdout == expected
    assert result.stderr == b""


def test_json_files(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    first = tmp_path / "first.json"
    second = tmp_path / "second.json"
    output = tmp_path / "output.cbor"
    first.write_text('{"a": 10}', encoding="utf-8")
    second.write_text('[13, "\u4e2d\u6587"]', encoding="utf-8")
    monkeypatch.setattr(
        "sys.argv", ["cbor2", "--format=json", "-o", str(output), str(first), str(second)]
    )
    cbor2.tool.main()
    assert output.read_bytes() == b"\xa1\x61a\x0a\x82\x0d\x66\xe4\xb8\xad\xe6\x96\x87"


def test_json_stdin_file(monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    output = tmp_path / "output.cbor"
    stdin = TextIOWrapper(BytesIO(b"[10, 13]"))
    monkeypatch.setattr("sys.argv", ["cbor2", "--format=json", "-o", str(output), "-"])
    monkeypatch.setattr("sys.stdin", stdin)
    cbor2.tool.main()
    assert output.read_bytes() == b"\x82\x0a\x0d"
    assert not stdin.closed


@pytest.mark.parametrize("document", [b"{", b"{} {}", b"", b'"\xff"', b'"\\ud800"'])
def test_json_invalid(document: bytes, monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    output = tmp_path / "output.cbor"
    stdin = TextIOWrapper(BytesIO(document))
    monkeypatch.setattr("sys.argv", ["cbor2", "--format=json", "-o", str(output)])
    monkeypatch.setattr("sys.stdin", stdin)
    with pytest.raises(SystemExit, match="^-: "):
        cbor2.tool.main()

    assert output.read_bytes() == b""
    assert not stdin.closed


@pytest.mark.parametrize(
    "option", ["--decode", "--pretty", "--sort-keys", "--sequence", "--tag-ignore=24"]
)
def test_json_incompatible_options(option: str, monkeypatch: MonkeyPatch, tmp_path: Path) -> None:
    output = tmp_path / "output.cbor"
    output.write_bytes(b"existing output")
    monkeypatch.setattr("sys.argv", ["cbor2", "--format=json", option, "-o", str(output)])
    with pytest.raises(SystemExit) as exc:
        cbor2.tool.main()

    assert exc.value.code == 2
    assert output.read_bytes() == b"existing output"
