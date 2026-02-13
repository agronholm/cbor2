import sys
from collections.abc import Callable, Iterator, Mapping
from datetime import tzinfo
from typing import _KT, IO, Any, Generic, TypeAlias, TypeVar, _T_co, _VT_co

if sys.version_info >= (3, 11):
    from typing import Self
else:
    from typing_extensions import Self

_T = TypeVar("_T")

TagHook: TypeAlias = Callable[[CBORDecoder, CBORTag], Any]
ObjectHook: TypeAlias = Callable[[CBORDecoder, dict[Any, Any]], Any]
EncoderHook: TypeAlias = Callable[[CBOREncoder, Any], Any]

class CBOREncoder:
    datetime_as_timestamp: bool
    timezone: tzinfo | None
    value_sharing: bool
    default: EncoderHook | None
    canonical: bool
    date_as_datetime: bool
    string_referencing: bool
    indefinite_containers: bool
    def __new__(
        cls,
        fp: IO[bytes],
        *,
        datetime_as_timestamp: bool = ...,
        timezone: tzinfo | None = ...,
        value_sharing: bool = ...,
        default: EncoderHook | None = ...,
        canonical: bool = ...,
        date_as_datetime: bool = ...,
        string_referencing: bool = ...,
        indefinite_containers: bool = ...,
    ) -> Self: ...
    fp: IO[bytes]
    def flush(self) -> None: ...
    def write(self, buf: bytes, /) -> None: ...
    def encode(self, obj: object, /) -> None: ...
    def encode_to_bytes(self, obj: object, /) -> bytes: ...
    def encode_length(self, major_tag: int, length: int | None) -> None: ...
    def encode_break(self) -> None: ...

class CBORDecoder:
    fp: IO[bytes]
    tag_hook: TagHook | None
    object_hook: ObjectHook | None
    str_errors: str
    read_size: int
    immutable: bool
    def __new__(
        cls,
        fp: IO[bytes],
        *,
        tag_hook: TagHook | None = ...,
        object_hook: ObjectHook | None = ...,
        str_errors: str = ...,
        read_size: int = ...,
    ) -> Self: ...
    def decode(self) -> Any: ...
    def decode_from_bytes(self, buf: bytes, /) -> Any: ...
    def set_shareable(self, value: object, /) -> None: ...
    def read(self, amount: int) -> bytes: ...

class CBORError(Exception): ...
class CBOREncodeError(CBORError): ...
class CBOREncodeTypeError(CBOREncodeError, TypeError): ...
class CBOREncodeValueError(CBOREncodeError, ValueError): ...
class CBORDecodeError(CBORError): ...
class CBORDecodeValueError(CBORDecodeError, ValueError): ...
class CBORDecodeEOF(CBORDecodeError, EOFError): ...

class FrozenDict(Generic[_KT, _VT_co], Mapping[_KT, _VT_co]):
    def __new__(cls, *args: Any) -> Self: ...
    def __getitem__(self, key: _KT, /) -> _VT_co:
        pass

    def __len__(self) -> int:
        pass

    def __iter__(self) -> Iterator[_T_co]:
        pass

class CBORTag:
    tag: int
    value: Any

    def __new__(cls, tag: int, value: Any) -> Self: ...
    def __eq__(self, other: object) -> bool: ...
    def __ne__(self, other: object) -> bool: ...
    def __lt__(self, other: object) -> bool: ...
    def __le__(self, other: object, /) -> bool: ...
    def __gt__(self, other: object, /) -> bool: ...
    def __ge__(self, other: object, /) -> bool: ...

class CBORSimpleValue:
    value: int

    def __new__(cls, value: int) -> Self: ...
    def __eq__(self, other: object) -> bool: ...
    def __ne__(self, other: object) -> bool: ...
    def __lt__(self, other: object) -> bool: ...
    def __le__(self, other: object, /) -> bool: ...
    def __gt__(self, other: object, /) -> bool: ...
    def __ge__(self, other: object, /) -> bool: ...

class BreakMarkerType: ...
class UndefinedType: ...

undefined: UndefinedType
break_marker: BreakMarkerType

encoders: dict[type, Callable[[CBOREncoder, Any], None]]
major_decoders: dict[int, Callable[[CBORDecoder], Any]]
semantic_decoders: dict[int, Callable[[CBORDecoder, CBORTag], Any]]

def dump(
    obj: object,
    fp: IO[bytes],
    *,
    datetime_as_timestamp: bool = False,
    timezone: tzinfo | None = None,
    value_sharing: bool = False,
    default: EncoderHook | None = None,
    canonical: bool = False,
    date_as_datetime: bool = False,
    string_referencing: bool = False,
    indefinite_containers: bool = False,
) -> None: ...
def dumps(
    obj: object,
    *,
    datetime_as_timestamp: bool = False,
    timezone: tzinfo | None = None,
    value_sharing: bool = False,
    default: EncoderHook | None = None,
    canonical: bool = False,
    date_as_datetime: bool = False,
    string_referencing: bool = False,
    indefinite_containers: bool = False,
) -> bytes: ...
def load(
    fp: IO[bytes],
    *,
    tag_hook: TagHook | None = None,
    object_hook: ObjectHook | None = None,
    str_errors: str = "strict",
) -> Any: ...
def loads(
    data: bytes,
    *,
    tag_hook: TagHook | None = None,
    object_hook: ObjectHook | None = None,
    str_errors: str = "strict",
) -> Any: ...
def shareable_encoder(
    wraps: Callable[[CBOREncoder, _T], None], /
) -> Callable[[CBOREncoder, _T], None]: ...
