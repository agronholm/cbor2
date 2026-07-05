import sys
from typing import TypeAlias, final

if sys.version_info >= (3, 11):
    from typing import Self
else:
    from typing_extensions import Self

@final
class Integer:
    value: int
    def __new__(cls, value: int) -> Self: ...

@final
class ByteString:
    value: bytes
    length: int
    def __new__(cls, value: bytes, length: int = ...) -> Self: ...

@final
class TextString:
    value: str
    length: int
    def __new__(cls, value: str, length: int = ...) -> Self: ...

@final
class ByteStringStart:
    def __new__(cls) -> Self: ...

@final
class TextStringStart:
    def __new__(cls) -> Self: ...

@final
class ArrayStart:
    length: int | None
    def __new__(cls, length: int | None = ...) -> Self: ...

@final
class MapStart:
    length: int | None
    def __new__(cls, length: int | None = ...) -> Self: ...

@final
class Tag:
    number: int
    def __new__(cls, number: int) -> Self: ...

@final
class Simple:
    value: int
    def __new__(cls, value: int) -> Self: ...

@final
class Float:
    value: float
    def __new__(cls, value: float) -> Self: ...

@final
class Boolean:
    value: bool
    def __new__(cls, value: bool) -> Self: ...

@final
class Null:
    def __new__(cls) -> Self: ...

@final
class Undefined:
    def __new__(cls) -> Self: ...

@final
class Break:
    def __new__(cls) -> Self: ...

@final
class MoreType: ...

MORE: MoreType

Token: TypeAlias = (
    Integer
    | ByteString
    | TextString
    | ByteStringStart
    | TextStringStart
    | ArrayStart
    | MapStart
    | Tag
    | Simple
    | Float
    | Boolean
    | Null
    | Undefined
    | Break
)
