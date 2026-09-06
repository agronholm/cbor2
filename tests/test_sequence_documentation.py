from __future__ import annotations

import doctest
from collections.abc import Callable, Iterator
from io import BufferedReader, BytesIO
from pathlib import Path
from typing import Any, cast

import pytest

from cbor2 import CBORDecodeEOF, CBORDecodeError, dumps


@pytest.fixture
def read_sequence() -> Callable[[BufferedReader[BytesIO]], Iterator[Any]]:
    path = Path(__file__).parent.parent / "docs" / "usage.rst"
    test = doctest.DocTestParser().get_doctest(path.read_text(), {}, "usage", str(path), 0)
    result = doctest.DocTestRunner().run(test, clear_globs=False)
    assert result.failed == 0
    assert result.attempted > 0
    return cast(
        "Callable[[BufferedReader[BytesIO]], Iterator[Any]]", test.globs["iter_cbor_sequence"]
    )


@pytest.mark.parametrize("size", [0, 1, 4095, 4096, 8193])
def test_sequence_boundaries(
    read_sequence: Callable[[BufferedReader[BytesIO]], Iterator[Any]], size: int
) -> None:
    objects = [None, "x" * size, {"value": size}, [1, 2]]
    with BufferedReader(BytesIO(b"".join(dumps(obj) for obj in objects))) as fp:
        assert list(read_sequence(fp)) == objects
    with BufferedReader(BytesIO()) as fp:
        assert list(read_sequence(fp)) == []


@pytest.mark.parametrize("suffix", [b"\x18", b"\x63ab", b"\x82\x01"])
def test_sequence_truncated_object(
    read_sequence: Callable[[BufferedReader[BytesIO]], Iterator[Any]], suffix: bytes
) -> None:
    with BufferedReader(BytesIO(dumps("complete") + suffix)) as fp:
        objects = read_sequence(fp)
        assert next(objects) == "complete"
        with pytest.raises(CBORDecodeEOF):
            next(objects)


def test_sequence_invalid_object(
    read_sequence: Callable[[BufferedReader[BytesIO]], Iterator[Any]],
) -> None:
    with BufferedReader(BytesIO(dumps(1) + b"\x1c")) as fp:
        objects = read_sequence(fp)
        assert next(objects) == 1
        with pytest.raises(CBORDecodeError):
            next(objects)
