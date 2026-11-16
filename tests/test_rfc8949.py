from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest

from cbor2 import CBORDecodeError, loads

VECTOR_DIR = Path(__file__).parent / "vectors"
APPENDIX_A = json.loads(VECTOR_DIR.joinpath("appendix_a.json").read_text())
APPENDIX_F1 = json.loads(VECTOR_DIR.joinpath("appendix_f1.json").read_text())
INVALID_ENCODINGS = {vector["hex"] for vector in APPENDIX_F1}
APPENDIX_A_VALID = [vector for vector in APPENDIX_A if vector["hex"] not in INVALID_ENCODINGS]
APPENDIX_A_INVALID = [vector for vector in APPENDIX_A if vector["hex"] in INVALID_ENCODINGS]


@pytest.mark.parametrize("vector", APPENDIX_A_VALID, ids=lambda vector: vector["hex"])
def test_rfc8949_appendix_a(vector: dict[str, Any]) -> None:
    decoded = loads(bytes.fromhex(vector["hex"]))
    if "decoded" in vector:
        assert decoded == vector["decoded"]


@pytest.mark.parametrize(
    "vector",
    [
        *(pytest.param(vector, id=f"appendix-a/{vector['hex']}") for vector in APPENDIX_A_INVALID),
        *(pytest.param(vector, id=f"appendix-f1/{vector['hex']}") for vector in APPENDIX_F1),
    ],
)
def test_rfc8949_invalid_items(vector: dict[str, Any]) -> None:
    with pytest.raises(CBORDecodeError):
        loads(bytes.fromhex(vector["hex"]))
