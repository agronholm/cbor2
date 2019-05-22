import pytest

from cbor2.types import FrozenDict


def test_tag_repr(impl):
    assert repr(impl.CBORTag(600, 'blah')) == "CBORTag(600, 'blah')"


def test_tag_init(impl):
    with pytest.raises(TypeError):
        impl.CBORTag('foo', 'bar')


def test_tag_attr(impl):
    tag = impl.CBORTag(1, 'foo')
    assert tag.tag == 1
    assert tag.value == 'foo'


def test_tag_compare(impl):
    tag1 = impl.CBORTag(1, 'foo')
    tag2 = impl.CBORTag(1, 'foo')
    tag3 = impl.CBORTag(2, 'bar')
    tag4 = impl.CBORTag(2, 'baz')
    assert tag1 is not tag2
    assert tag1 == tag2
    assert not (tag1 == tag3)
    assert tag1 != tag3
    assert tag3 >= tag2
    assert tag3 > tag2
    assert tag2 < tag3
    assert tag2 <= tag3
    assert tag4 >= tag3
    assert tag4 > tag3
    assert tag3 < tag4
    assert tag3 <= tag4
    assert not tag1 == (1, 'foo')


def test_tag_recursive(impl):
    tag = impl.CBORTag(1, None)
    tag.value = tag
    assert repr(tag) == 'CBORTag(1, ...)'
    assert tag is tag.value
    assert tag == tag.value
    assert not (tag != tag.value)


def test_tag_repr(impl):
    assert repr(impl.CBORTag(600, 'blah')) == "CBORTag(600, 'blah')"


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
