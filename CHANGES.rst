Version history
===============

This library adheres to `Semantic Versioning <http://semver.org/>`_.

**3.0.4** (2016-09-24)

- Fixed TypeError when trying to encode extension types (regression introduced in 3.0.3)

**3.0.3** (2016-09-23)

- No changes, just re-releasing due to git tagging screw-up

**3.0.2** (2016-09-23)

- Fixed decoding failure for datetimes with microseconds (tag 0)

**3.0.1** (2016-08-08)

- Fixed error in the cyclic structure detection code that could mistake one container for
  another, sometimes causing a bogus error about cyclic data structures where there was none

**3.0.0** (2016-07-03)

- **BACKWARD INCOMPATIBLE** Encoder callbacks now receive three arguments: the encoder instance,
  the value to encode and a file-like object. The callback must must now either write directly to
  the file-like object or call another encoder callback instead of returning an iterable.
- **BACKWARD INCOMPATIBLE** Semantic decoder callbacks now receive four arguments: the decoder
  instance, the primitive value, a file-like object and the shareable index for the decoded value.
  Decoders that support value sharing must now set the raw value at the given index in
  ``decoder.shareables``.
- **BACKWARD INCOMPATIBLE** Removed support for iterative encoding (``CBOREncoder.encode()`` is no
  longer a generator function and always returns ``None``)
- Significantly improved performance (encoder ~30 % faster, decoder ~60 % faster)
- Fixed serialization round-trip for ``undefined`` (simple type #23)
- Added proper support for value sharing in callbacks

**2.0.0** (2016-06-11)

- **BACKWARD INCOMPATIBLE** Deserialize unknown tags as ``CBORTag`` objects so as not to lose
  information
- Fixed error messages coming from nested structures

**1.1.0** (2016-06-10)

- Fixed deserialization of cyclic structures

**1.0.0** (2016-06-08)

- Initial release
