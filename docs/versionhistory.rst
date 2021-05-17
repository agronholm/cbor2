Version history
===============

.. currentmodule:: cbor2

This library adheres to `Semantic Versioning <http://semver.org/>`_.

**5.3.0** (2021-05-18)

- Removed support for Python < 3.6

**5.2.0** (2020-09-30)

- Final version tested with Python 2.7 and 3.5
- README: Announce deprecation of Python 2.7, 3.5
- README: More detail and examples
- Bugfix: Fix segfault on loading huge arrays with C-backend (Sekenre)
- Build system: Allow packagers to force C-backend building or disable using env var (jameshilliard)
- Feature: :module:`cbor2.tool` Command line diagnostic tool (Sekenre)
- Feature: Ignore semantic tag used for file magic 55799 AKA "Self-Described CBOR" (kalcutter)

**5.1.2** (2020-07-21)

- Bugfix: Refcount bug in C lib causing intermittent segfaults on shutdown (tdryer)

**5.1.1** (2020-07-03)

- Build system: Making C lib optional if it fails to compile (chiefnoah)
- Build system: Better Glibc version detection (Sekenre and JayH5)
- Tests: Positive and negative bignums (kalcutter)
- Bugfix: Fractional seconds parsing in datetimes (kalcutter)

**5.1.0** (2020-03-18)

- Minor API change :class:`CBORSimpleValue` is now a subclass of namedtuple and allows
  all numeric comparisons. This brings functional parity between C and Python modules.
- Fixes for C-module on big-endian systems including floating point decoding, smallint encoding,
  and boolean argument handling. Tested on s390x and MIPS32.
- Increase version requred of setuptools during install due to unicode errors.

**5.0.1** (2020-01-21)

- Fix deprecation warning on python 3.7, 3.8 (mariano54)
- Minor documentation tweaks

**5.0.0** (2020-01-20)

- **BACKWARD INCOMPATIBLE** CBOR does not have a bare DATE type, encoding dates as datetimes
  is disabled by default (PR by Changaco)
- **BACKWARD INCOMPATIBLE** :meth:`~CBORDecoder.set_shareable` only takes the instance to share, not
  the shareable's index
- **BACKWARD INCOMPATIBLE** :exc:`CBORError` now descends from :exc:`Exception` rather than
  :exc:`ValueError`; however, subordinate exceptions now descend from :exc:`ValueError` (where
  appropriate) so most users should notice no difference
- **BACKWARD INCOMPATIBLE** :class:`CBORDecoder` can now raise :exc:`CBORDecodeEOF` which descends
  from :exc:`EOFError` supporting streaming applications
- Optional Pure C implementation by waveform80 that functions identically to the pure Python
  implementation with further contributions from: toravir, jonashoechst, Changaco
- Drop Python 3.3 and 3.4 support from the build process; they should still work if built from
  source but are no longer officially supported
- Added support for encoding and decoding :class:`ipaddress.IPv4Address`,
  :class:`ipaddress.IPv6Address`, :class:`ipaddress.IPv4Network`, and :class:`ipaddress.IPv6Network`
  (semantic tags 260 and 261)

**4.2.0** (2020-01-10)

- **BROKEN BUILD** Removed

**4.1.2** (2018-12-10)

- Fixed bigint encoding taking quadratic time
- Fixed overflow errors when encoding floating point numbers in canonical mode
- Improved decoder performance for dictionaries
- Minor documentation tweaks

**4.1.1** (2018-10-14)

- Fixed encoding of negative :class:`decimal.Decimal` instances (PR by Sekenre)

**4.1.0** (2018-05-27)

- Added canonical encoding (via ``canonical=True``) (PR by Sekenre)
- Added support for encoding/decoding sets (semantic tag 258) (PR by Sekenre)
- Added support for encoding `FrozenDict` (hashable dict) as map keys or set elements (PR by
  Sekenre)

**4.0.1** (2017-08-21)

- Fixed silent truncation of decoded data if there are not enough bytes in the stream for an exact
  read (:exc:`CBORDecodeError` is now raised instead)

**4.0.0** (2017-04-24)

- **BACKWARD INCOMPATIBLE** Value sharing has been disabled by default, for better compatibility
  with other implementations and better performance (since it is rarely needed)
- **BACKWARD INCOMPATIBLE** Replaced the ``semantic_decoders`` decoder option with the
  :attr:`CBORDecoder.tag_hook` option
- **BACKWARD INCOMPATIBLE** Replaced the ``encoders`` encoder option with the
  :attr:`CBOREncoder.default` option
- **BACKWARD INCOMPATIBLE** Factored out the file object argument (``fp``) from all callbacks
- **BACKWARD INCOMPATIBLE** The encoder no longer supports every imaginable type implementing the
  ``Sequence`` or ``Map`` interface, as they turned out to be too broad
- Added the :attr:`CBORDecoder.object_hook` option for decoding dicts into complex objects (intended
  for situations where JSON compatibility is required and semantic tags cannot be used)
- Added encoding and decoding of simple values (:class:`CBORSimpleValue`) (contributed by Jerry
  Lundström)
- Replaced the decoder for bignums with a simpler and faster version (contributed by orent)
- Made all relevant classes and functions available directly in the :mod:`cbor2` namespace
- Added proper documentation

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
- **BACKWARD INCOMPATIBLE** Removed support for iterative encoding (:meth:`CBOREncoder.encode` is no
  longer a generator function and always returns :data:`None`)
- Significantly improved performance (encoder ~30 % faster, decoder ~60 % faster)
- Fixed serialization round-trip for :data:`undefined` (simple type 23)
- Added proper support for value sharing in callbacks

**2.0.0** (2016-06-11)

- **BACKWARD INCOMPATIBLE** Deserialize unknown tags as :class:`CBORTag` objects so as not to lose
  information
- Fixed error messages coming from nested structures

**1.1.0** (2016-06-10)

- Fixed deserialization of cyclic structures

**1.0.0** (2016-06-08)

- Initial release
