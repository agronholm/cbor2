import pytest
import sys
import binascii
import base64
from unittest import mock
from io import StringIO, BytesIO, TextIOWrapper

import cbor2.tool


def test_stdin(monkeypatch, tmpdir):
    f = tmpdir.join('outfile')
    argv = ['-o', str(f)]
    inbuf = TextIOWrapper(BytesIO(binascii.unhexlify('02')))
    with monkeypatch.context() as m:
        m.setattr('sys.argv', [''] + argv)
        m.setattr('sys.stdin', inbuf)
        cbor2.tool.main()
        assert f.read() == '2\n'


def test_readfrom(monkeypatch, tmpdir):
    f = tmpdir.join('infile')
    outfile = tmpdir.join('outfile')
    f.write_binary(binascii.unhexlify('02'))
    argv = ['-o', str(outfile), str(f)]
    with monkeypatch.context() as m:
        m.setattr('sys.argv', [''] + argv)
        cbor2.tool.main()
        assert outfile.read() == '2\n'

