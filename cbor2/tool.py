r"""Command-line tool for CBOR diagnostics and testing

It converts CBOR data in raw binary or base64 encoding into a representation
that allows printing as JSON. This is a lossy transformation as each
datatype is converted into something that can be represented as a JSON
value.

Usage::

    $ echo a16568656c6c6f65776f726c64 | xxd -r -ps | python -m cbor2.tool --pretty
    {
        "hello": "world"
    }
    $ echo ggEC | python -m cbor2.tool -d
    [1, 2]
    $ python -m cbor2.tool -d tests/examples.cbor.b64
    {...}

It can be used in a pipeline with json processing tools like `jq`_ to allow syntax coloring,
field extraction and more.

CBOR data items concatenated into a sequence can be decoded also::

    $ cat tests/examples.cbor.b64 tests/examples.cbor.b64 | python -m cbor2.tool -d --sequence
    {...}
    {...}

Multiple files can also be sent to a single output file::

    $ python -m cbor2.tool -o all_files.json file1.cbor file2.cbor ... fileN.cbor

.. _jq: https://stedolan.github.io/jq/
"""
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

    extra_encoders = OrderedDict(
        [
            (ipaddress.IPv4Address, lambda x: str(x)),
            (ipaddress.IPv6Address, lambda x: str(x)),
            (ipaddress.IPv4Network, lambda x: str(x)),
            (ipaddress.IPv6Network, lambda x: str(x)),
        ]
    )
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


def key_to_str(d, dict_ids=None):
    dict_ids = set(dict_ids or [])
    rval = {}
    if not isinstance(d, dict):
        if isinstance(d, CBORSimpleValue):
            v = 'cbor_simple:{:d}'.format(d.value)
            return v
        if isinstance(d, (tuple, list, set)):
            if id(d) in dict_ids:
                raise ValueError("Cannot convert self-referential data to JSON")
            else:
                dict_ids.add(id(d))
            v = [key_to_str(x, dict_ids) for x in d]
            dict_ids.remove(id(d))
            return v
        else:
            return d

    if id(d) in dict_ids:
        raise ValueError("Cannot convert self-referential data to JSON")
    else:
        dict_ids.add(id(d))

    for k, v in d.items():
        if isinstance(k, bytes):
            k = base64.b64encode(k).decode('ascii')
        if isinstance(k, CBORSimpleValue):
            k = 'cbor_simple:{:d}'.format(k.value)
        if isinstance(k, (FrozenDict, frozenset, tuple)):
            k = str(k)
        if isinstance(v, dict):
            v = key_to_str(v, dict_ids)
        elif isinstance(v, (tuple, list, set)):
            v = [key_to_str(x, dict_ids) for x in v]
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
        '-o', '--outfile', type=argparse.FileType('w'), help='output file', default=sys.stdout
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
                except (ValueError, EOFError) as e:  # pragma: no cover
                    raise SystemExit(e)


if __name__ == '__main__':  # pragma: no cover
    main()
