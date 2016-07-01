Version history
===============

This library adheres to `Semantic Versioning <http://semver.org/>`_.

**3.0.0**

- **BACKWARD INCOMPATIBLE** Encoder callbacks must now either use ``encoder.fp.write()`` or call
  another encoder callback instead of returning an iterable
- **BACKWARD INCOMPATIBLE** ``CBOREncoder.encode()`` is no longer a generator function and now
  always returns ``None``
- Significantly improved encoder performance (~33 %)
- Fixed serialization round-trip for ``undefined`` (simple type #23)

**2.0.0** (2016-06-11)

- **BACKWARD INCOMPATIBLE** Deserialize unknown tags as ``CBORTag`` objects so as not to lose
  information
- Fixed error messages coming from nested structures

**1.1.0** (2016-06-10)

- Fixed deserialization of cyclic structures

**1.0.0** (2016-06-08)

- Initial release
