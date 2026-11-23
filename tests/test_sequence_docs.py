import asyncio
import re
from collections.abc import AsyncIterator, Callable, Iterator
from io import BufferedIOBase, BufferedReader, BytesIO, UnsupportedOperation
from pathlib import Path
from textwrap import dedent
from typing import SupportsIndex, cast

import pytest

from cbor2 import CBORDecodeEOF, CBORDecodeError, dumps


class NonSeekableBytesIO(BytesIO):
    def seekable(self) -> bool:
        return False

    def seek(self, offset: SupportsIndex, whence: int = 0, /) -> int:
        raise UnsupportedOperation("not seekable")


def run_example(name: str, namespace: dict[str, object]) -> None:
    usage = Path(__file__).resolve().parents[1] / "docs" / "usage.rst"
    match = re.search(rf"(?m)^    :name: {name}\n\n((?:    .*\n|\n)+)", usage.read_text())
    assert match is not None, f"Missing documentation example: {name}"
    # Execute the checked-in documentation, not a separate copy of the recipe.
    exec(compile(dedent(match[1]), str(usage), "exec"), namespace)


@pytest.mark.parametrize("stream_class", [BytesIO, NonSeekableBytesIO])
@pytest.mark.parametrize("buffer_size", [7, 8192])
@pytest.mark.parametrize(
    "values",
    [[], [1], [1, "hello", {"enabled": True}, None], [b"x" * 20000, "end", 2]],
    ids=["empty", "single", "mixed", "large"],
)
def test_sequence_recipe(
    stream_class: type[BytesIO], buffer_size: int, values: list[object]
) -> None:
    namespace: dict[str, object] = {}
    run_example("sequence-reader", namespace)
    decode_sequence = cast(
        Callable[[BufferedIOBase], Iterator[object]], namespace["decode_sequence"]
    )
    with BufferedReader(
        stream_class(b"".join(dumps(value) for value in values)), buffer_size=buffer_size
    ) as fp:
        assert list(decode_sequence(fp)) == values


@pytest.mark.parametrize("stream_class", [BytesIO, NonSeekableBytesIO])
@pytest.mark.parametrize("buffer_size", [7, 8192])
@pytest.mark.parametrize(
    "tail, error",
    [(b"\x62a", CBORDecodeEOF), (b"\x1c", CBORDecodeError)],
    ids=["truncated", "malformed"],
)
def test_sequence_recipe_rejects_corrupt_tail(
    stream_class: type[BytesIO], buffer_size: int, tail: bytes, error: type[Exception]
) -> None:
    namespace: dict[str, object] = {}
    run_example("sequence-reader", namespace)
    decode_sequence = cast(
        Callable[[BufferedIOBase], Iterator[object]], namespace["decode_sequence"]
    )
    with BufferedReader(stream_class(dumps(1) + tail), buffer_size=buffer_size) as fp:
        values = decode_sequence(fp)
        assert next(values) == 1
        with pytest.raises(error):
            list(values)


def test_sequence_file_example(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    (tmp_path / "sequence.cbor").write_bytes(dumps(1) + dumps("hello"))
    monkeypatch.chdir(tmp_path)
    namespace: dict[str, object] = {}
    run_example("sequence-reader", namespace)
    run_example("sequence-file", namespace)
    assert capsys.readouterr().out == "1\nhello\n"


def test_sequence_memory_example(capsys: pytest.CaptureFixture[str]) -> None:
    namespace: dict[str, object] = {}
    run_example("sequence-reader", namespace)
    run_example("sequence-memory", namespace)
    assert capsys.readouterr().out == "[1, 'hello', None]\n"


def read_async_example(payload: bytes, chunk_size: int) -> list[object]:
    namespace: dict[str, object] = {}
    run_example("async-sequence-reader", namespace)
    decode_async_sequence = cast(
        Callable[[asyncio.StreamReader], AsyncIterator[object]],
        namespace["decode_async_sequence"],
    )

    async def read() -> list[object]:
        reader = asyncio.StreamReader()

        async def feed() -> None:
            for offset in range(0, len(payload), chunk_size):
                reader.feed_data(payload[offset : offset + chunk_size])
                await asyncio.sleep(0)
            reader.feed_eof()

        feeder = asyncio.create_task(feed())
        try:
            return [value async for value in decode_async_sequence(reader)]
        finally:
            await feeder

    return asyncio.run(read())


@pytest.mark.parametrize("chunk_size", [1, 7, 65536])
@pytest.mark.parametrize(
    "values",
    [[], [None, False, 0], [1, {"text": "caf\u00e9"}, [2, 3]], [b"x" * 300, "tail"]],
    ids=["empty", "falsey", "nested", "large"],
)
def test_async_sequence_recipe(chunk_size: int, values: list[object]) -> None:
    assert read_async_example(b"".join(dumps(value) for value in values), chunk_size) == values


@pytest.mark.parametrize("chunk_size", [1, 65536])
@pytest.mark.parametrize(
    "tail,error",
    [(b"\x62a", CBORDecodeEOF), (b"\x1c", CBORDecodeError)],
    ids=["truncated", "malformed"],
)
def test_async_sequence_recipe_rejects_corrupt_tail(
    chunk_size: int, tail: bytes, error: type[Exception]
) -> None:
    with pytest.raises(error):
        read_async_example(dumps(False) + tail, chunk_size)
