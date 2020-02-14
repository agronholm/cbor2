import argparse
import json
import sys
import base64
import io
import re
import decimal
import fractions
import uuid
from datetime import datetime
from collections import OrderedDict
from . import load, CBORDecoder
from .types import FrozenDict

try:
    from _cbor2 import CBORTag, undefined, CBORSimpleValue
except ImportError:
    from .types import CBORTag, undefined, CBORSimpleValue

try:
    import ipaddress
    extra_encoders = OrderedDict([
        (ipaddress.IPv4Address, lambda x: str(x)),
        (ipaddress.IPv6Address, lambda x: str(x)),
        (ipaddress.IPv4Network, lambda x: str(x)),
        (ipaddress.IPv6Network, lambda x: str(x)),
        ])
except ImportError:
    extra_encoders = OrderedDict()


default_encoders = OrderedDict(
    [
        (bytes, lambda x: base64.b64encode(x).decode('ascii')),
        (decimal.Decimal, lambda x: str(x)),
        (FrozenDict, lambda x: str(dict(x))),
        (CBORSimpleValue, lambda x: 'cbor_simple:{:d}'.format(x.value)),
        (type(undefined), lambda x: 'cbor:undef'),
        (datetime, lambda x: x.isoformat()),
        (fractions.Fraction, lambda x: str(x)),
        (uuid.UUID, lambda x: x.urn),
        (CBORTag, lambda x: {'CBORTag:{:d}'.format(x.tag): x.value}),
        (set, lambda x: list(x)),
        (re.compile('').__class__, lambda x: x.pattern),
    ]
)
default_encoders.update(extra_encoders)


class DefEncoder(json.JSONEncoder):
    def default(self, v):
        obj_type = v.__class__
        encoder = default_encoders.get(obj_type)
        if encoder:
            return encoder(v)
        return json.JSONEncoder.default(self, v)


def iterdecode(f):
    decoder = CBORDecoder(f)
    while True:
        try:
            yield decoder.decode()
        except EOFError:
            return


def key_to_str(d):
    rval = {}
    if not isinstance(d, dict):
        if isinstance(d, CBORSimpleValue):
            v = 'cbor_simple:{:d}'.format(d.value)
            return v
        if isinstance(d, (tuple, list, set)):
            v = [key_to_str(x) for x in d]
            return v
        else:
            return d

    for k, v in d.items():
        if isinstance(k, bytes):
            k = base64.b64encode(k).decode('ascii')
        if isinstance(k, CBORSimpleValue):
            k = 'cbor_simple:{:d}'.format(k.value)
        if isinstance(k, (FrozenDict, frozenset, tuple)):
            k = str(k)
        if isinstance(v, dict):
            v = key_to_str(v)
        elif isinstance(v, (tuple, list, set)):
            v = [key_to_str(x) for x in v]
        rval[k] = v
    return rval


def main():
    prog = 'python -m cbor2.tool'
    description = (
        'A simple command line interface for cbor2 module '
        'to validate and pretty-print CBOR objects.'
    )
    parser = argparse.ArgumentParser(prog=prog, description=description)
    parser.add_argument(
        '-o',
        '--outfile',
        type=argparse.FileType('w'),
        help='output file',
        default=sys.stdout,
    )
    parser.add_argument(
        'infiles',
        nargs='*',
        type=argparse.FileType('rb'),
        help='Collection of CBOR files to process or - for stdin',
    )
    parser.add_argument(
        '--sort-keys',
        action='store_true',
        default=False,
        help='sort the output of dictionaries alphabetically by key',
    )
    parser.add_argument(
        '--pretty', action='store_true', default=False, help='indent the output to look good'
    )
    parser.add_argument(
        '--sequence',
        action='store_true',
        default=False,
        help='Parse a sequence of concatenated CBOR items',
    )
    parser.add_argument(
        '-d',
        '--decode',
        action='store_true',
        default=False,
        help='CBOR data is base64 encoded (handy for stdin)',
    )
    options = parser.parse_args()

    outfile = options.outfile
    sort_keys = options.sort_keys
    pretty = options.pretty
    sequence = options.sequence
    decode = options.decode
    infiles = options.infiles or [sys.stdin.buffer]
    with outfile:
        for infile in infiles:
            with infile:
                if decode:
                    infile = io.BytesIO(base64.b64decode(infile.read()))
                try:
                    if sequence:
                        objs = iterdecode(infile)
                    else:
                        objs = (load(infile),)
                    for obj in objs:
                        json.dump(
                            key_to_str(obj),
                            outfile,
                            sort_keys=sort_keys,
                            indent=(None, 4)[pretty],
                            cls=DefEncoder,
                        )
                        outfile.write('\n')
                except ValueError as e:
                    raise SystemExit(e)


if __name__ == '__main__':
    main()
