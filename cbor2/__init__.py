from .decoder import CBORDecoder as CBORDecoder
from .decoder import load as load
from .decoder import loads as loads
from .encoder import CBOREncoder as CBOREncoder
from .encoder import dump as dump
from .encoder import dumps as dumps
from .encoder import shareable_encoder as shareable_encoder
from .types import CBORDecodeEOF as CBORDecodeEOF
from .types import CBORDecodeError as CBORDecodeError
from .types import CBORDecodeValueError as CBORDecodeValueError
from .types import CBOREncodeError as CBOREncodeError
from .types import CBOREncodeTypeError as CBOREncodeTypeError
from .types import CBOREncodeValueError as CBOREncodeValueError
from .types import CBORError as CBORError
from .types import CBORSimpleValue as CBORSimpleValue
from .types import CBORTag as CBORTag
from .types import undefined as undefined

try:
    from _cbor2 import *  # noqa: F403
except ImportError:
    # Couldn't import the optimized C version; ignore the failure and leave the
    # pure Python implementations in place.
    pass
else:
    # The pure Python implementations are replaced with the optimized C
    # variants, but we still need to create the encoder dictionaries for the C
    # variant here (this is much simpler than doing so in C, and doesn't affect
    # overall performance as it's a one-off initialization cost).
    def _init_cbor2():
        from collections import OrderedDict

        import _cbor2

        from .encoder import canonical_encoders, default_encoders
        from .types import CBORSimpleValue, CBORTag, undefined  # noqa: F811

        _cbor2.default_encoders = OrderedDict(
            [
                (
                    (
                        _cbor2.CBORSimpleValue
                        if type_ is CBORSimpleValue
                        else _cbor2.CBORTag
                        if type_ is CBORTag
                        else type(_cbor2.undefined)
                        if type_ is type(undefined)
                        else type_
                    ),
                    getattr(_cbor2.CBOREncoder, method.__name__),
                )
                for type_, method in default_encoders.items()
            ]
        )
        _cbor2.canonical_encoders = OrderedDict(
            [
                (
                    (
                        _cbor2.CBORSimpleValue
                        if type_ is CBORSimpleValue
                        else _cbor2.CBORTag
                        if type_ is CBORTag
                        else type(_cbor2.undefined)
                        if type_ is type(undefined)
                        else type_
                    ),
                    getattr(_cbor2.CBOREncoder, method.__name__),
                )
                for type_, method in canonical_encoders.items()
            ]
        )

    _init_cbor2()
    del _init_cbor2
