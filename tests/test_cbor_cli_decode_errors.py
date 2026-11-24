import subprocess
import sys
from pathlib import Path

import pytest


@pytest.mark.parametrize(
    "payload, options, expected_output",
    [(b"\x1c", [], b""), (b"\xff", [], b""), (b"\x01\x1c", ["--sequence"], b"1\n")],
)
@pytest.mark.skipif(sys.platform == "emscripten", reason="Subprocesses unavailable")
def test_invalid_cbor_is_reported_without_a_traceback(
    tmp_path: Path, payload: bytes, options: list[str], expected_output: bytes
) -> None:
    infile = tmp_path / "input.cbor"
    infile.write_bytes(payload)
    result = subprocess.run(
        [sys.executable, "-m", "cbor2.tool", *options, str(infile)],
        capture_output=True,
        check=False,
    )
    assert result.returncode != 0
    assert result.stdout.replace(b"\r\n", b"\n") == expected_output
    assert result.stderr
    assert b"Traceback" not in result.stderr
    assert b"unknown unsigned integer subtype" in result.stderr or b"break code" in result.stderr
