from cbor2.types import CBORTag


def test_tag_repr():
    assert repr(CBORTag(600, 'blah')) == "CBORTag(600, 'blah')"


def test_tag_equals():
    tag1 = CBORTag(500, ['foo'])
    tag2 = CBORTag(500, ['foo'])
    tag3 = CBORTag(500, ['bar'])
    assert tag1 == tag2
    assert not tag1 == tag3
    assert not tag1 == 500
