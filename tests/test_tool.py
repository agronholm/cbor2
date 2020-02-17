from __future__ import unicode_literals
import pytest
import sys
import binascii
import json
from io import BytesIO, TextIOWrapper

import cbor2.tool


@pytest.mark.parametrize(
    'value, expected',
    [
        ((1, 2, 3), [1, 2, 3]),
        ({b"\x01\x02\x03": "b"}, {"AQID": "b"}),
        ({"dict": {"b": 17}}, {"dict": {"b": 17}}),
    ],
    ids=['tuple', 'byte_key', 'recursion'],
)
def test_key_to_str(value, expected):
    assert cbor2.tool.key_to_str(value) == expected


def test_default():
    with pytest.raises(TypeError):
        json.dumps(BytesIO(b''), cls=cbor2.tool.DefEncoder)


@pytest.mark.parametrize(
    'payload', ["D81CA16162D81CA16161D81D00", "d81c81d81c830102d81d00"], ids=['dict', 'list']
)
def test_self_referencing(payload):
    decoded = cbor2.loads(binascii.unhexlify(payload))
    with pytest.raises(ValueError, match="Cannot convert self-referential data to JSON"):
        cbor2.tool.key_to_str(decoded)


def test_nonrecursive_ref():
    payload = 'd81c83d81ca26162d81ca16161016163d81d02d81d01d81d01'
    decoded = cbor2.loads(binascii.unhexlify(payload))
    result = cbor2.tool.key_to_str(decoded)
    expected = [
        {"b": {"a": 1}, "c": {"a": 1}},
        {"b": {"a": 1}, "c": {"a": 1}},
        {"b": {"a": 1}, "c": {"a": 1}},
    ]
    assert result == expected


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
    argv = ['-o', str(f)]
    inbuf = TextIOWrapper(BytesIO(binascii.unhexlify('42C2C2')))
    expected = '"wsI="\n' if sys.version_info >= (3, 3) else b'"\\u00c2\\u00c2"\n'
    with monkeypatch.context() as m:
        m.setattr('sys.argv', [''] + argv)
        m.setattr('sys.stdin', inbuf)
        cbor2.tool.main()
        assert f.read() == expected


@pytest.mark.skipif(
    sys.version_info < (3, 6), reason="No ipaddress module and simple value is unhashable"
)
def test_dtypes_from_file(monkeypatch, tmpdir):
    infile = 'tests/examples.cbor.b64'
    outfile = tmpdir.join('outfile.json')
    argv = ['--sort-keys', '-d', '-o', str(outfile), infile]
    with monkeypatch.context() as m:
        m.setattr('sys.argv', [''] + argv)
        cbor2.tool.main()
        assert outfile.read().startswith('{"bytes": [')
