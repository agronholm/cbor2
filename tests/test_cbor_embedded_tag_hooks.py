import json
import sys
from pathlib import Path

import pytest
from pytest import MonkeyPatch

import cbor2
from cbor2.tool import main, tag_hook


@pytest.mark.parametrize("depth", [1, 2])
@pytest.mark.parametrize("ignored", [False, True])
def test_diagnostic_hooks_apply_to_embedded_cbor(
    monkeypatch: MonkeyPatch, tmp_path: Path, depth: int, ignored: bool
) -> None:
    value = cbor2.CBORTag(6000, "inner")
    for _ in range(depth):
        value = cbor2.CBORTag(24, cbor2.dumps(value))
    infile = tmp_path / "input.cbor"
    outfile = tmp_path / "output.json"
    infile.write_bytes(cbor2.dumps(value))
    options = ["--tag-ignore", "6000"] if ignored else []
    monkeypatch.setattr(sys, "argv", ["cbor2", *options, "-o", str(outfile), str(infile)])
    main()
    expected = "inner" if ignored else {"CBORTag:6000": "inner"}
    assert json.loads(outfile.read_text(encoding="utf-8")) == expected


def test_explicitly_ignored_embedded_tag_keeps_its_payload() -> None:
    payload = cbor2.dumps(cbor2.CBORTag(6000, "inner"))
    assert tag_hook(cbor2.CBORTag(24, payload), False, (24, 6000)) == payload
