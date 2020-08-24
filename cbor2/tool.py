r"""Command-line tool for CBOR diagnostics and testing

It converts CBOR data in raw binary or base64 encoding into a representation
that allows printing as JSON. This is a lossy transformation as each
datatype is converted into something that can be represented as a JSON
value.

Usage::

    # Pass hexadecimal through xxd.
    $ echo a16568656c6c6f65776f726c64 | xxd -r -ps | python -m cbor2.tool --pretty
    {
        "hello": "world"
    }
    # Decode Base64 directly
    $ echo ggEC | python -m cbor2.tool --decode
    [1, 2]
    # Read from a file encoded in Base64
    $ python -m cbor2.tool -d tests/examples.cbor.b64
    {...}

It can be used in a pipeline with json processing tools like `jq`_ to allow syntax coloring,
field extraction and more.

CBOR data items concatenated into a sequence can be decoded also::

    $ echo ggECggMEggUG | python -m cbor2.tool -d --sequence
    [1, 2]
    [3, 4]
    [5, 6]

Multiple files can also be sent to a single output file::

    $ python -m cbor2.tool -o all_files.json file1.cbor file2.cbor ... fileN.cbor

.. _jq: https://stedolan.github.io/jq/
"""
import argparse
import base64
import decimal
import fractions
import io
import json
import re
import sys
import uuid
from collections import OrderedDict
from datetime import datetime
from functools import partial

from . import CBORDecoder, load
from .types import FrozenDict

try:
    from _cbor2 import CBORSimpleValue, CBORTag, undefined
except ImportError:
    from .types import CBORSimpleValue, CBORTag, undefined

try:
    import ipaddress

    default_encoders = OrderedDict(
        [
            (ipaddress.IPv4Address, str),
            (ipaddress.IPv6Address, str),
            (ipaddress.IPv4Network, str),
            (ipaddress.IPv6Network, str),
        ]
    )
except ImportError:
    default_encoders = OrderedDict()


default_encoders.update(
    [
        (bytes, lambda x: x.decode(encoding='utf-8', errors='backslashreplace')),
        (decimal.Decimal, str),
        (FrozenDict, lambda x: str(dict(x))),
        (CBORSimpleValue, lambda x: 'cbor_simple:{:d}'.format(x.value)),
        (type(undefined), lambda x: 'cbor:undef'),
        (datetime, lambda x: x.isoformat()),
        (fractions.Fraction, str),
        (uuid.UUID, lambda x: x.urn),
        (CBORTag, lambda x: {'CBORTag:{:d}'.format(x.tag): x.value}),
        (set, list),
        (re.compile('').__class__, lambda x: x.pattern),
    ]
)


def tag_hook(decoder, tag, ignore_tags=set()):
    if tag.tag in ignore_tags:
        return tag.value
    if tag.tag == 24:
        return decoder.decode_from_bytes(tag.value)
    else:
        if decoder.immutable:
            return 'CBORtag:{}:{}'.format(tag.tag, tag.value)
        return tag


class DefaultEncoder(json.JSONEncoder):
    def default(self, v):
        obj_type = v.__class__
        encoder = default_encoders.get(obj_type)
        if encoder:
            return encoder(v)
        return json.JSONEncoder.default(self, v)


def iterdecode(f, *args, **kwargs):
    decoder = CBORDecoder(f, *args, **kwargs)
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
            k = k.decode(encoding='utf-8', errors='backslashreplace')
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
        '-o', '--outfile', type=str, help='output file', default='-'
    )
    parser.add_argument(
        'infiles',
        nargs='*',
        type=argparse.FileType('rb'),
        help='Collection of CBOR files to process or - for stdin',
    )
    parser.add_argument(
        '-k',
        '--sort-keys',
        action='store_true',
        default=False,
        help='sort the output of dictionaries alphabetically by key',
    )
    parser.add_argument(
        '-p',
        '--pretty', action='store_true', default=False, help='indent the output to look good'
    )
    parser.add_argument(
        '-s',
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
    parser.add_argument(
        '-i',
        '--tag-ignore',
        type=str,
        help='Comma separated list of tags to ignore and only return the value',
    )
    options = parser.parse_args()

    outfile = options.outfile
    sort_keys = options.sort_keys
    pretty = options.pretty
    sequence = options.sequence
    decode = options.decode
    infiles = options.infiles or [sys.stdin]

    closefd = True
    if outfile == '-':
        outfile = 1
        closefd = False

    if sys.version_info < (3, 3):
        opener = dict(mode='wb', closefd=closefd)
        dumpargs = dict(ensure_ascii=True, encoding='raw_unicode_escape')
    else:
        opener = dict(mode='w', encoding='utf-8', errors='backslashescape', closefd=closefd)
        dumpargs = dict(ensure_ascii=False)

    if options.tag_ignore:
        ignore_s = options.tag_ignore.split(',')
        droptags = set(int(n) for n in ignore_s if (len(n) and n[0].isdigit()))
    else:
        droptags = set()

    my_hook = partial(tag_hook, ignore_tags=droptags)

    with io.open(outfile, **opener) as outfile:
        for infile in infiles:
            if hasattr(infile, 'buffer') and not decode:
                infile = infile.buffer
            with infile:
                if decode:
                    infile = io.BytesIO(base64.b64decode(infile.read()))
                try:
                    if sequence:
                        objs = iterdecode(infile, tag_hook=my_hook)
                    else:
                        objs = (load(infile, tag_hook=my_hook),)
                    for obj in objs:
                        json.dump(
                            key_to_str(obj),
                            outfile,
                            sort_keys=sort_keys,
                            indent=(None, 4)[pretty],
                            cls=DefaultEncoder,
                            **dumpargs
                        )
                        outfile.write('\n')
                except (ValueError, EOFError) as e:  # pragma: no cover
                    raise SystemExit(e)


if __name__ == '__main__':  # pragma: no cover
    main()
