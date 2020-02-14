import pytest
import sys
import binascii
import base64
from io import BytesIO, TextIOWrapper

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

def test_b64(monkeypatch, tmpdir):
    f = tmpdir.join('outfile')
    argv = ['-d', '-o', str(f)]
    inbuf = TextIOWrapper(BytesIO(b'oQID'))
    with monkeypatch.context() as m:
        m.setattr('sys.argv', [''] + argv)
        m.setattr('sys.stdin', inbuf)
        cbor2.tool.main()
        assert f.read() == '{"2": 3}\n'

def test_stream(monkeypatch, tmpdir):
    f = tmpdir.join('outfile')
    argv = ['--sequence', '-o', str(f)]
    inbuf = TextIOWrapper(BytesIO(binascii.unhexlify('0203')))
    with monkeypatch.context() as m:
        m.setattr('sys.argv', [''] + argv)
        m.setattr('sys.stdin', inbuf)
        cbor2.tool.main()
        assert f.read() == '2\n3\n'

def test_embed_bytes(monkeypatch, tmpdir):
    f = tmpdir.join('outfile')
    argv = [ '-o', str(f)]
    inbuf = TextIOWrapper(BytesIO(binascii.unhexlify('43010203')))
    expected = '"AQID"\n' if sys.version_info >= (3, 3) else '"\u0001\u0002\u0003"\n'
    with monkeypatch.context() as m:
        m.setattr('sys.argv', [''] + argv)
        m.setattr('sys.stdin', inbuf)
        cbor2.tool.main()
        assert f.read() == expected

@pytest.mark.skipif(sys.version_info < (3, 3), reason="No ipaddress module")
def test_dtypes_from_file(monkeypatch, tmpdir):
    infile = 'tests/examples.cbor.b64'
    outfile = tmpdir.join('outfile.json')
    argv = [ '--sort-keys', '-d', '-o', str(outfile), infile ]
    with monkeypatch.context() as m:
        m.setattr('sys.argv', [''] + argv)
        cbor2.tool.main()
        assert outfile.read().startswith('{"bytes": [')

