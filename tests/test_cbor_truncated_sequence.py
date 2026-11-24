import io
import subprocess
import sys
from pathlib import Path

import pytest

from cbor2 import CBORDecodeEOF
from cbor2.tool import iterdecode


class NonSeekable(io.BytesIO):
    def seekable(self) -> bool:
        return False

    def seek(self, offset: int, whence: int = 0) -> int:
        raise io.UnsupportedOperation("seek")

    def tell(self) -> int:
        raise io.UnsupportedOperation("tell")


@pytest.mark.parametrize("stream_type", [io.BytesIO, NonSeekable])
@pytest.mark.parametrize("suffix", [b"\x18", b"\x63ab", b"\x82\x02", b"\x9f\x02", b"\xa1\x02"])
def test_truncated_sequence_item_is_an_error(stream_type: type[io.BytesIO], suffix: bytes) -> None:
    stream = stream_type(b"\x01" + suffix)
    values = iterdecode(stream)
    assert next(values) == 1
    with pytest.raises(CBORDecodeEOF):
        next(values)

    assert not stream.closed


@pytest.mark.parametrize("data, expected", [(b"", []), (b"\x01\x02\x03", [1, 2, 3])])
@pytest.mark.parametrize("stream_type", [io.BytesIO, NonSeekable])
def test_clean_end_of_sequence_keeps_stream_open(
    stream_type: type[io.BytesIO], data: bytes, expected: list[int]
) -> None:
    stream = stream_type(data)
    assert list(iterdecode(stream)) == expected
    assert not stream.closed


def test_existing_buffered_stream_is_kept_open() -> None:
    with io.BufferedReader(io.BytesIO(b"\x01\x18")) as stream:
        values = iterdecode(stream)
        assert next(values) == 1
        with pytest.raises(CBORDecodeEOF):
            next(values)

        assert not stream.closed


def test_early_close_preserves_the_seekable_stream_position() -> None:
    stream = io.BytesIO(b"\x01\x02\x03")
    values = iterdecode(stream)
    assert next(values) == 1
    values.close()
    assert not stream.closed
    assert stream.read() == b"\x02\x03"


@pytest.mark.skipif(sys.platform == "emscripten", reason="Subprocesses unavailable")
def test_cli_fails_on_a_truncated_final_item(tmp_path: Path) -> None:
    infile = tmp_path / "input.cbor"
    infile.write_bytes(b"\x01\x63ab")
    result = subprocess.run(
        [sys.executable, "-m", "cbor2.tool", "--sequence", str(infile)],
        capture_output=True,
        check=False,
    )
    assert result.returncode != 0
    assert result.stdout.replace(b"\r\n", b"\n") == b"1\n"
    assert b"premature end" in result.stderr
