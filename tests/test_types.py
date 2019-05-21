import pytest

from cbor2.types import FrozenDict


def test_tag_repr(impl):
    assert repr(impl.CBORTag(600, 'blah')) == "CBORTag(600, 'blah')"


def test_tag_equals(impl):
    tag1 = impl.CBORTag(500, ['foo'])
    tag2 = impl.CBORTag(500, ['foo'])
    tag3 = impl.CBORTag(500, ['bar'])
    assert tag1 == tag2
    assert not tag1 == tag3
    assert not tag1 == 500


def test_simple_value_repr(impl):
    assert repr(impl.CBORSimpleValue(1)) == "CBORSimpleValue(value=1)"


def test_simple_value_equals(impl):
    tag1 = impl.CBORSimpleValue(1)
    tag2 = impl.CBORSimpleValue(1)
    tag3 = impl.CBORSimpleValue(21)
    assert tag1 == tag2
    assert tag1 == 1
    assert not tag2 == "21"
    assert tag1 != tag3
    assert tag1 != 21
    assert tag2 != "21"


def test_simple_value_too_big(impl):
    with pytest.raises(TypeError) as exc:
        impl.CBORSimpleValue(256)
        assert str(exc.value) == 'simple value out of range (0..255)'


def test_frozendict():
    assert len(FrozenDict({1: 2, 3: 4})) == 2
    assert repr(FrozenDict({1: 2})) == "FrozenDict({1: 2})"
