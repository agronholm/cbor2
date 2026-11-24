import json
import subprocess
import sys
from pathlib import Path

import pytest

import cbor2
from cbor2.tool import main


@pytest.mark.parametrize("option", ["-i", "--tag-ignore"])
@pytest.mark.parametrize("tags", [" 6000 , 6001 ", "6000, 6001", ",6000,,6001,6000,"])
def test_ignored_tags_allow_whitespace_and_empty_entries(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, option: str, tags: str
) -> None:
    infile = tmp_path / "input.cbor"
    outfile = tmp_path / "output.json"
    infile.write_bytes(cbor2.dumps([cbor2.CBORTag(6000, "one"), cbor2.CBORTag(6001, "two")]))
    monkeypatch.setattr(sys, "argv", ["cbor2", option, tags, "-o", str(outfile), str(infile)])
    main()
    assert json.loads(outfile.read_text(encoding="utf-8")) == ["one", "two"]


@pytest.mark.parametrize("tag", [6000, 2**64 - 1])
def test_valid_tag_numbers(monkeypatch: pytest.MonkeyPatch, tmp_path: Path, tag: int) -> None:
    infile = tmp_path / "input.cbor"
    outfile = tmp_path / "output.json"
    infile.write_bytes(cbor2.dumps(cbor2.CBORTag(tag, "value")))
    monkeypatch.setattr(
        sys, "argv", ["cbor2", "--tag-ignore=" + str(tag), "-o", str(outfile), str(infile)]
    )
    main()
    assert json.loads(outfile.read_text(encoding="utf-8")) == "value"


@pytest.mark.parametrize("tags", ["garbage", "6000oops", "-1", str(2**64)])
@pytest.mark.skipif(sys.platform == "emscripten", reason="Subprocesses unavailable")
def test_invalid_tag_numbers_are_usage_errors(tmp_path: Path, tags: str) -> None:
    infile = tmp_path / "input.cbor"
    infile.write_bytes(b"\x01")
    result = subprocess.run(
        [sys.executable, "-m", "cbor2.tool", "--tag-ignore=" + tags, str(infile)],
        capture_output=True,
        check=False,
    )
    assert result.returncode == 2
    assert not result.stdout
    assert b"--tag-ignore" in result.stderr
    assert b"Traceback" not in result.stderr
