import sys
from collections.abc import Callable, Mapping
from typing import Any, TypeAlias

from ._cbor2 import CBORDecodeEOF as CBORDecodeEOF
from ._cbor2 import CBORDecodeError as CBORDecodeError
from ._cbor2 import CBORDecoder as CBORDecoder
from ._cbor2 import CBORDecodeValueError as CBORDecodeValueError
from ._cbor2 import CBOREncodeError as CBOREncodeError
from ._cbor2 import CBOREncoder as CBOREncoder
from ._cbor2 import CBOREncodeTypeError as CBOREncodeTypeError
from ._cbor2 import CBOREncodeValueError as CBOREncodeValueError
from ._cbor2 import CBORError as CBORError
from ._cbor2 import CBORSimpleValue as CBORSimpleValue
from ._cbor2 import CBORTag as CBORTag
from ._cbor2 import dump as dump
from ._cbor2 import dumps as dumps
from ._cbor2 import load as load
from ._cbor2 import loads as loads
from ._cbor2 import shareable_decoder as shareable_decoder
from ._cbor2 import shareable_encoder as shareable_encoder
from ._cbor2 import undefined as undefined

if sys.hexversion < 51314855:
    from ._cbor2 import frozendict as frozendict

TagHook: TypeAlias = Callable[[CBORTag, bool], Any]
SemanticDecoderCallback: TypeAlias = Callable[[Any, bool], Any]
ObjectHook: TypeAlias = Callable[[Mapping[Any, Any], bool], Any]
EncoderHook: TypeAlias = Callable[[CBOREncoder, Any], Any]
ShareableDecoderCallback: TypeAlias = Callable[[Any], Any]
ShareableDecoderInitializer: TypeAlias = Callable[[bool], tuple[Any, ShareableDecoderCallback]]

del Any, Callable, Mapping, TypeAlias, sys
