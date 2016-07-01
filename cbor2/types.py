class CBORTag(object):
    __slots__ = 'tag', 'value'

    def __init__(self, tag, value):
        self.tag = tag
        self.value = value

    def __eq__(self, other):
        if isinstance(other, CBORTag):
            return self.tag == other.tag and self.value == other.value
        return NotImplemented

    def __repr__(self):
        return 'CBORTag({self.tag}, {self.value!r})'.format(self=self)


class UndefinedType(object):
    __slots__ = ()


undefined = UndefinedType()
break_marker = object()
