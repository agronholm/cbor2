import argparse
import json
import sys
import base64
import io
from datetime import datetime
from collections import OrderedDict
from . import load, CBORDecoder
from .types import *

import decimal, fractions, uuid, ipaddress

default_encoders = OrderedDict([
    (bytes,                         lambda x: base64.b64encode(x).decode('ascii')),
    (decimal.Decimal,               lambda x: str(x)),
    (FrozenDict,                    lambda x: str(dict(x))),
    (type(undefined),               lambda x: 'cbor:undef'),
    (datetime,                      lambda x: x.isoformat()),
    (fractions.Fraction,            lambda x: str(x)),
    (uuid.UUID,              lambda x: x.urn),
    (ipaddress.IPv4Address,  lambda x: str(x)),
    (ipaddress.IPv6Address,  lambda x: str(x)),
    (ipaddress.IPv4Network,  lambda x: str(x)),
    (ipaddress.IPv6Network,  lambda x: str(x)),
    (CBORSimpleValue,               lambda x: "cbor_simple:{:d}".format(x.value)),
    (CBORTag,                       lambda x: 'cbor_tag:{:d}:{}'.format( x.tag, x.value )),
    (set,                           lambda x: list(x)),
    (frozenset,                     lambda x: str(x)),
])

class DefEncoder(json.JSONEncoder):
    def default(self, v):
        obj_type = v.__class__
        encoder = default_encoders.get(obj_type)
        if encoder:
            return encoder(v)
        else:
            return str(v)
        return json.JSONEncoder.default(self, v)

def iterdecode(f):
    decoder = CBORDecoder(f)
    while True:
        try:
            yield decoder.decode()
        except EOFError:
            return


def main():
    prog = 'python -m cbor2.tool'
    description = ('A simple command line interface for cbor2 module '
                   'to validate and pretty-print CBOR objects.')
    parser = argparse.ArgumentParser(prog=prog, description=description)
    parser.add_argument('-o', '--outfile',
                        type=argparse.FileType('w', encoding="utf-8"),
                        help='output file',
                        default=sys.stdout)
    parser.add_argument('infiles', nargs='*',
                        type=argparse.FileType('rb'),
                        help='Collection of CBOR files to process or - for stdin')
    parser.add_argument('--sort-keys', action='store_true', default=False,
                        help='sort the output of dictionaries alphabetically by key')
    parser.add_argument('--pretty', action='store_true', default=False,
                        help='indent the output to look good')
    parser.add_argument('--sequence', action='store_true', default=False,
                        help='Parse a sequence of concatenated CBOR items')
    parser.add_argument('-d', '--decode', action='store_true', default=False,
                        help='CBOR data is base64 encoded (handy for stdin)')
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
                        objs = (load(infile), )
                    for obj in objs:
                        json.dump(obj, outfile, sort_keys=sort_keys, indent=(None, 4)[pretty], cls=DefEncoder)
                        outfile.write('\n')
                except ValueError as e:
                    raise SystemExit(e)


if __name__ == '__main__':
    main()

