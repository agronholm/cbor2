import platform

import pytest
from cbor2 import CBORSimpleValue, CBORTag, FrozenDict, break_marker, undefined


class TestUndefined:
    def test_bool(self) -> None:
        assert not undefined

    def test_repr(self) -> None:
        assert repr(undefined) == "undefined"

    @pytest.mark.skipif(
        platform.python_implementation() == "PyPy", reason="PyPy does not raise TypeError"
    )
    def test_singleton(self) -> None:
        with pytest.raises(TypeError, match="cannot create 'cbor2.UndefinedType' instances"):
            type(undefined)()


class TestBreakMarker:
    def test_bool(self) -> None:
        assert break_marker

    def test_repr(self) -> None:
        assert repr(break_marker) == "break_marker"

    @pytest.mark.skipif(
        platform.python_implementation() == "PyPy", reason="PyPy does not raise TypeError"
    )
    def test_singleton(self) -> None:
        with pytest.raises(TypeError, match="cannot create 'cbor2.BreakMarkerType' instances"):
            type(break_marker)()


class TestCBORTag:
    def test_bad_args(self) -> None:
        with pytest.raises(TypeError):
            CBORTag("foo", "bar")  # type: ignore[arg-type]

    def test_attr(self) -> None:
        tag = CBORTag(1, "foo")
        assert tag.tag == 1
        assert tag.value == "foo"

    def test_compare(self) -> None:
        tag1 = CBORTag(1, "foo")
        tag2 = CBORTag(1, "foo")
        tag3 = CBORTag(2, "bar")
        tag4 = CBORTag(2, "baz")
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

    def test_compare_unimplemented(self) -> None:
        tag = CBORTag(1, "foo")
        assert not tag == (1, "foo")
        with pytest.raises(TypeError):
            tag <= (1, "foo")

    def test_recursive_repr(self) -> None:
        some_list: list[CBORTag] = []
        tag = CBORTag(1, some_list)
        some_list.append(tag)
        assert repr(tag) == "CBORTag(1, [CBORTag(1, [...])])"

    def test_non_hashable(self) -> None:
        with pytest.raises(RuntimeError, match="This CBORTag is not hashable"):
            hash(CBORTag(1, []))

    def test_repr(self) -> None:
        assert repr(CBORTag(600, "blah")) == "CBORTag(600, 'blah')"


class TestCBORSimpleValue:
    def test_equals(self) -> None:
        tag1 = CBORSimpleValue(1)
        tag2 = CBORSimpleValue(1)
        tag3 = CBORSimpleValue(21)
        tag4 = CBORSimpleValue(99)
        assert tag1 == tag2
        assert tag1 == 1
        assert not tag2 == "21"
        assert tag1 != tag3
        assert tag1 != 21
        assert tag2 != "21"
        assert tag4 > tag1
        assert tag4 >= tag3
        assert 99 <= tag4
        assert 100 > tag4
        assert tag4 <= 100
        assert 2 < tag4
        assert tag4 >= 99
        assert tag1 <= tag4

    def test_ordering(self) -> None:
        randints = [9, 7, 3, 8, 4, 0, 2, 5, 6, 1]
        expected = [CBORSimpleValue(v) for v in range(10)]
        disordered = [CBORSimpleValue(v) for v in randints]
        assert expected == sorted(disordered)
        assert expected == sorted(randints)

    @pytest.mark.parametrize("value", [-1, 24, 31, 256])
    def test_simple_value_out_of_range(self, value: int) -> None:
        with pytest.raises(ValueError, match="simple value out of range"):
            CBORSimpleValue(value)

    def test_repr(self) -> None:
        assert repr(CBORSimpleValue(1)) == "CBORSimpleValue(1)"


class TestFrozenDict:
    def test_get(self) -> None:
        obj = FrozenDict[int, int]({1: 2})
        assert obj.get(1) == 2
        assert obj.get(3) is None
        assert obj.get(3, "foo") == "foo"

    def test_len(self) -> None:
        assert len(FrozenDict({1: 2, 3: 4})) == 2

    def test_repr(self) -> None:
        assert repr(FrozenDict({1: 2})) == "FrozenDict({1: 2})"
