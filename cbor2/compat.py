import sys


if sys.version_info.major < 3:
    from datetime import tzinfo, timedelta

    class timezone(tzinfo):
        def __init__(self, offset):
            self.offset = offset

        def utcoffset(self, dt):
            return self.offset

        def dst(self, dt):
            return timedelta(0)

        def tzname(self, dt):
            return 'UTC+00:00'

    def as_unicode(string):
        return string.decode('utf-8')

    PY2 = True
    byte_as_integer = ord
    timezone.utc = timezone(timedelta(0))
    xrange = xrange  # noqa
    long = long  # noqa
    unicode = unicode  # noqa
    iteritems = dict.iteritems
else:
    from datetime import timezone

    def byte_as_integer(bytestr):
        return bytestr[0]

    def as_unicode(string):
        return string

    PY2 = False
    xrange = range  # noqa
    long = int  # noqa
    unicode = str  # noqa
    iteritems = dict.items
