import json
import sys
from pathlib import Path

import pytest
from pytest import MonkeyPatch

import cbor2
from cbor2.tool import key_to_str, main


@pytest.mark.parametrize(
    "value, expected",
    [
        ({2: "two", "a": "text"}, {"2": "two", "a": "text"}),
        ({True: "yes", "a": "text"}, {"true": "yes", "a": "text"}),
        ({None: "none", "a": "text"}, {"null": "none", "a": "text"}),
        ({1.5: "half", "a": "text"}, {"1.5": "half", "a": "text"}),
        ({"items": [{False: 1, "b": 2}]}, {"items": [{"false": 1, "b": 2}]}),
    ],
)
def test_sort_keys_accepts_mixed_scalar_keys(
    monkeypatch: MonkeyPatch, tmp_path: Path, value: object, expected: object
) -> None:
    infile = tmp_path / "input.cbor"
    outfile = tmp_path / "output.json"
    infile.write_bytes(cbor2.dumps(value))
    for options in ([], ["--sort-keys"]):
        monkeypatch.setattr(sys, "argv", ["cbor2", *options, "-o", str(outfile), str(infile)])
        main()
        text = outfile.read_text(encoding="utf-8")
        assert json.loads(text) == expected
        if options:
            assert text == json.dumps(expected, sort_keys=True) + "\n"


@pytest.mark.parametrize("key, spelling", [(1, "1"), (True, "true"), (None, "null")])
def test_colliding_json_key_spellings_are_rejected(key: object, spelling: str) -> None:
    with pytest.raises(ValueError, match="same JSON representation"):
        key_to_str({key: "first", spelling: "second"})
